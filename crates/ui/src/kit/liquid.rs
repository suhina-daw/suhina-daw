// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opt-in surface materials. The native host supplies the live GPU backdrop; views only
//! describe the surface and paint sharp controls afterward. No audio-thread code lives here.

use std::sync::Arc;

use egui::{Color32, Context, Id, Painter, Rect, Stroke, StrokeKind, TextureId, pos2};

use super::{motion, theme};

/// The theme panel colour before main-window background opacity is applied.
pub fn set_panel_base(ctx: &Context, color: Color32) {
    ctx.data_mut(|data| data.insert_temp(Id::new("liquid-panel-base"), color));
}

pub fn panel_base(ctx: &Context) -> Color32 {
    ctx.data(|data| data.get_temp::<Color32>(Id::new("liquid-panel-base"))).unwrap_or_else(theme::window_background)
}

/// A bounded app-owned background image the host may include in its offscreen renderer.
#[derive(Clone)]
pub struct BackgroundImage {
    pub id: TextureId,
    pub pixels: Arc<egui::ColorImage>,
}

pub fn set_background_image(ctx: &Context, image: Option<BackgroundImage>) {
    ctx.data_mut(|data| data.insert_temp(Id::new("liquid-background-image"), image));
}

pub fn background_image(ctx: &Context) -> Option<BackgroundImage> {
    ctx.data(|data| data.get_temp::<Option<BackgroundImage>>(Id::new("liquid-background-image"))).flatten()
}

/// Surfaces can be enabled independently without changing a colour theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// 0 disables the material; 100 is the strongest supported treatment.
    pub panels: u8,
    pub tracks: u8,
    pub clips: u8,
}

/// The affected surface; track and clip materials retain their content colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Panel,
    Track,
    Clip,
}

/// An opaque, gamma-encoded backdrop registered with the host's egui renderer.
#[derive(Clone, Copy)]
pub struct Backdrop {
    pub texture: TextureId,
    /// The same frame before blur, for the narrow refracting rim.
    pub sharp_texture: Option<TextureId>,
    pub screen: Rect,
}

/// Host-side GPU work, always requested on the UI thread before the floating panel is drawn.
pub trait Provider: Send + Sync {
    fn capture(&self, ctx: &Context) -> Option<Backdrop>;
    /// One content-only substrate, independent of the modal capture and its textures.
    fn capture_surfaces(&self, _ctx: &Context, _shapes: Vec<egui::epaint::ClippedShape>) -> Option<Backdrop> {
        None
    }
    /// The window's own layers, blurred, for the menus floating over them. A see-through window
    /// keeps its alpha in the image, so the desktop's blur still shows where the window does.
    fn capture_behind(
        &self,
        _ctx: &Context,
        _shapes: Vec<egui::epaint::ClippedShape>,
        _see_through: bool,
    ) -> Option<Backdrop> {
        None
    }
}

#[derive(Clone)]
struct Host(Arc<dyn Provider>);

const OPTIONS: &str = "liquid-options";
const HOST: &str = "liquid-host";
const BACKDROP: &str = "liquid-backdrop";
const ACCESSIBLE: &str = "liquid-effects-allowed";
const SURFACES: &str = "liquid-frame-surfaces";
const GRADIENTS: &str = "liquid-gradients";

#[derive(Clone, Default)]
struct Surfaces {
    source: Vec<egui::epaint::ClippedShape>,
    lenses: Vec<Lens>,
}

#[derive(Clone)]
struct Lens {
    layer: egui::LayerId,
    clip: Rect,
    opacity: f32,
    index: egui::layers::ShapeIdx,
    rect: Rect,
    strength: f32,
    mesh: Option<egui::Mesh>,
}

/// Clear frame-local placeholders. The source never contains text or previous-frame lenses.
pub fn begin_frame(ctx: &Context, gradients: super::gradient::Gradients) {
    ctx.data_mut(|data| {
        data.insert_temp(Id::new(SURFACES), Surfaces::default());
        data.insert_temp(Id::new(GRADIENTS), gradients);
        data.remove_temp::<Vec<Marked>>(Id::new(MARKED));
    });
}

/// Resolve all content lenses once the visible substrate is complete, before modal chrome.
pub fn finish_surfaces(ctx: &Context) {
    let mut surfaces = ctx.data_mut(|data| data.remove_temp::<Surfaces>(Id::new(SURFACES))).unwrap_or_default();
    if !surfaces.lenses.is_empty() && effects_allowed(ctx) {
        let host = ctx.data(|data| data.get_temp::<Host>(Id::new(HOST)));
        if let Some(backdrop) = host.and_then(|host| host.0.capture_surfaces(ctx, std::mem::take(&mut surfaces.source)))
        {
            for lens in &mut surfaces.lenses {
                let mesh = refraction_mesh(lens.rect, backdrop, lens.strength);
                let mut painter = ctx.layer_painter(lens.layer).with_clip_rect(lens.clip);
                painter.set_opacity(lens.opacity);
                painter.set(lens.index, mesh.clone());
                lens.mesh = Some(mesh);
            }
        }
    }
    ctx.data_mut(|data| data.insert_temp(Id::new(SURFACES), surfaces));
}

const MARKED: &str = "liquid-backdrops";

/// A dialog's panel marked for the window's content, blurred, under it (`backdrop`).
#[derive(Clone)]
struct Marked {
    layer: egui::LayerId,
    clip: Rect,
    /// Two shapes reserved where the panel is painted: the cut, then the image.
    cut: egui::layers::ShapeIdx,
    image: egui::layers::ShapeIdx,
    rect: Rect,
    opacity: f32,
}

/// Marks `rect` for the window's content, blurred, under what `painter` paints next: a dialog's
/// panel, called just before its fill. `finish_backdrops` puts the image there once the frame is
/// painted.
pub fn backdrop(painter: &Painter, rect: Rect) {
    let ctx = painter.ctx();
    if !effects_allowed(ctx) || !available(ctx) || !rect.is_finite() {
        return;
    }
    let cut = painter.add(egui::Shape::Noop);
    let image = painter.add(egui::Shape::Noop);
    let marked =
        Marked { layer: painter.layer_id(), clip: painter.clip_rect(), cut, image, rect, opacity: painter.opacity() };
    ctx.data_mut(|data| {
        let all = data.get_temp_mut_or_default::<Vec<Marked>>(Id::new(MARKED));
        if all.len() < 64 {
            all.push(marked);
        }
    });
}

/// Menus, other popups and the dialogs that asked (`backdrop`) blur the window's content behind
/// them. Called once the frame is painted, before `kit::surface` swaps its backgrounds (the
/// capture cannot replay those).
pub fn finish_backdrops(ctx: &Context) {
    let marked = ctx.data_mut(|data| data.remove_temp::<Vec<Marked>>(Id::new(MARKED))).unwrap_or_default();
    if !effects_allowed(ctx) {
        return;
    }
    let Some(host) = ctx.data(|data| data.get_temp::<Host>(Id::new(HOST))) else { return };
    // A popup's frame (egui's menus, context menus and dropdowns) is the first shape of its
    // floating layer: the window fill with the window's outline.
    let visuals = ctx.global_style().visuals.clone();
    let order: Vec<egui::LayerId> = ctx.memory(|memory| memory.layer_ids().collect());
    let menus: Vec<(egui::LayerId, usize, Rect, f32)> = ctx.graphics(|layers| {
        order
            .iter()
            .filter(|layer| layer.order == egui::Order::Foreground)
            .filter_map(|layer| {
                let list = layers.get(*layer)?;
                let (index, first) =
                    list.all_entries().enumerate().find(|(_, clipped)| !matches!(clipped.shape, egui::Shape::Noop))?;
                // With a shadow, egui paints the frame as [shadow, frame].
                let frame = match &first.shape {
                    egui::Shape::Vec(parts) => parts.last()?,
                    shape => shape,
                };
                // While the popup fades in, egui scales its colours: compare them without alpha.
                match frame {
                    egui::Shape::Rect(frame)
                        if same_colour(frame.fill, visuals.window_fill)
                            && same_colour(frame.stroke.color, visuals.window_stroke.color)
                            && frame.stroke.width == visuals.window_stroke.width =>
                    {
                        let shown = f32::from(frame.fill.a()) / f32::from(visuals.window_fill.a().max(1));
                        Some((*layer, index, frame.rect, shown.clamp(0.0, 1.0)))
                    }
                    _ => None,
                }
            })
            .collect()
    });
    if suhina_plugins::trace::on() {
        suhina_plugins::trace::state("menu backdrop", format!("{} menus, {} dialogs", menus.len(), marked.len()));
    }
    if menus.is_empty() && marked.is_empty() {
        return;
    }
    let window: Vec<egui::LayerId> = order
        .iter()
        .copied()
        .filter(|layer| matches!(layer.order, egui::Order::Background | egui::Order::Middle))
        .collect();
    let transforms =
        window.iter().filter_map(|layer| ctx.layer_transform_to_global(*layer).map(|t| (*layer, t))).collect();
    // `drain` adds every layer, not only those named: the floating ones are emptied first.
    let shapes = ctx.graphics(|layers| {
        let mut copy = layers.clone();
        for layer in order.iter().filter(|layer| !window.contains(layer)) {
            *copy.entry(*layer) = egui::layers::PaintList::default();
        }
        copy.drain(&window, &transforms)
    });
    let see_through = super::surface::available(ctx);
    let Some(backdrop) = host.0.capture_behind(ctx, shapes, see_through) else {
        if suhina_plugins::trace::on() {
            suhina_plugins::trace::state("menu backdrop", "no capture".into());
        }
        return;
    };
    // In a see-through window the pixels are cleared first, so the sharp content under them does
    // not show through the image's alpha; while a menu or dialog fades in, the image fades in
    // over the content instead.
    let under = |rect: Rect, shown: f32| {
        let uv_min = (rect.min - backdrop.screen.min) / backdrop.screen.size();
        let uv_max = (rect.max - backdrop.screen.min) / backdrop.screen.size();
        let uv = Rect::from_min_max(uv_min.to_pos2(), uv_max.to_pos2());
        let cut = if shown >= 1.0 { super::surface::clear(ctx, rect) } else { None };
        (cut, egui::Shape::image(backdrop.texture, rect, uv, Color32::WHITE.gamma_multiply(shown)))
    };
    let dialogs: Vec<(Marked, Option<egui::Shape>, egui::Shape)> = marked
        .into_iter()
        .map(|marked| {
            let (cut, image) = under(marked.rect, marked.opacity.clamp(0.0, 1.0));
            (marked, cut, image)
        })
        .collect();
    let under: Vec<(egui::LayerId, usize, Vec<egui::Shape>)> = menus
        .into_iter()
        .map(|(layer, index, rect, shown)| {
            let (cut, image) = under(rect, shown);
            (layer, index, cut.into_iter().chain([image]).collect())
        })
        .collect();
    ctx.graphics_mut(|layers| {
        for (marked, cut, image) in dialogs {
            let list = layers.entry(marked.layer);
            if let Some(cut) = cut {
                list.set(marked.cut, marked.clip, cut);
            }
            list.set(marked.image, marked.clip, image);
        }
        for (layer, index, shapes) in under {
            let list = layers.entry(layer);
            let entries: Vec<egui::epaint::ClippedShape> = list.all_entries().cloned().collect();
            let mut rebuilt = egui::layers::PaintList::default();
            for (i, clipped) in entries.into_iter().enumerate() {
                if i == index {
                    for shape in &shapes {
                        rebuilt.add(clipped.clip_rect, shape.clone());
                    }
                }
                rebuilt.add(clipped.clip_rect, clipped.shape);
            }
            *list = rebuilt;
        }
    });
}

/// Whether two colours are the same but for their alpha.
fn same_colour(a: Color32, b: Color32) -> bool {
    let ([ar, ag, ab, aa], [br, bg, bb, ba]) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
    if aa == 0 || ba == 0 {
        return aa == ba;
    }
    // Scaling by an opacity rounds each channel a little.
    [(ar, br), (ag, bg), (ab, bb)].iter().all(|(x, y)| x.abs_diff(*y) <= 3)
}

/// Updated by the app when reduced effects or a high-contrast theme is selected.
pub fn set_effects_allowed(ctx: &Context, allowed: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::new(ACCESSIBLE), allowed));
}

fn effects_allowed(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(Id::new(ACCESSIBLE))).unwrap_or(true)
}

/// Install an optional native provider. A host without one keeps solid surfaces.
pub fn install(ctx: &Context, provider: Arc<dyn Provider>) {
    ctx.data_mut(|data| {
        data.insert_temp(Id::new(HOST), Host(provider));
    });
}

/// Whether the current host supports the experimental material renderer.
pub fn available(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<Host>(Id::new(HOST))).is_some()
}

/// Apply saved per-surface strengths live. Unsupported hosts retain solid surfaces.
pub fn configure(ctx: &Context, options: Options) {
    let options = if available(ctx) { options } else { Options::default() };
    ctx.data_mut(|data| data.insert_temp(Id::new(OPTIONS), options));
}

/// Forget the previous panel's image, then capture this frame before any modal chrome.
pub fn prepare(ctx: &Context) {
    ctx.data_mut(|data| data.remove::<Backdrop>(Id::new(BACKDROP)));
    let options = ctx.data(|data| data.get_temp::<Options>(Id::new(OPTIONS))).unwrap_or_default();
    if options.panels == 0 || !effects_allowed(ctx) {
        return;
    }
    let host = ctx.data(|data| data.get_temp::<Host>(Id::new(HOST)));
    // Panel capture replays bodies/foreground but excludes native content lenses: the dialog's
    // backdrop never holds glass drawn from another capture.
    let surfaces = ctx.data_mut(|data| data.remove_temp::<Surfaces>(Id::new(SURFACES))).unwrap_or_default();
    for lens in &surfaces.lenses {
        ctx.layer_painter(lens.layer).with_clip_rect(lens.clip).set(lens.index, egui::Shape::Noop);
    }
    let backdrop = host.and_then(|host| host.0.capture(ctx));
    for lens in &surfaces.lenses {
        if let Some(mesh) = &lens.mesh {
            let mut painter = ctx.layer_painter(lens.layer).with_clip_rect(lens.clip);
            painter.set_opacity(lens.opacity);
            painter.set(lens.index, mesh.clone());
        }
    }
    ctx.data_mut(|data| data.insert_temp(Id::new(SURFACES), surfaces));
    if let Some(backdrop) = backdrop {
        ctx.data_mut(|data| data.insert_temp(Id::new(BACKDROP), backdrop));
    }
}

/// Paint the base first, then this material, then the surface's text, waveform and controls.
/// Content lenses resolve against a shared foreground-free substrate at finish_surfaces.
/// Floating panels use the complete current UI backdrop. Tiny surfaces stay solid.
pub fn paint(painter: &Painter, rect: Rect, base: Color32, surface: Surface) {
    if !rect.is_finite()
        || rect.width() < 12.0
        || rect.height() < 12.0
        || !painter.clip_rect().intersects(rect)
        || !effects_allowed(painter.ctx())
    {
        return;
    }
    if super::native_css::background(painter, rect) {
        return;
    }
    let gradient = painter.ctx().data(|data| data.get_temp::<super::gradient::Gradients>(Id::new(GRADIENTS))).and_then(
        |gradients| match surface {
            Surface::Panel => gradients.panels,
            Surface::Track => gradients.tracks,
            Surface::Clip => gradients.clips,
        },
    );
    let options = painter.ctx().data(|data| data.get_temp::<Options>(Id::new(OPTIONS))).unwrap_or_default();
    let level = match surface {
        Surface::Panel => options.panels,
        Surface::Track => options.tracks,
        Surface::Clip => options.clips,
    };
    if level == 0 {
        if surface != Surface::Panel && (options.tracks > 0 || options.clips > 0) {
            painter.ctx().data_mut(|data| {
                let surfaces = data.get_temp_mut_or_default::<Surfaces>(Id::new(SURFACES));
                if surfaces.source.len() < 12288 {
                    let clip_rect = painter.clip_rect().intersect(rect);
                    surfaces.source.push(egui::epaint::ClippedShape {
                        clip_rect,
                        shape: egui::Shape::rect_filled(rect, 0.0, base),
                    });
                    if let Some(gradient) = &gradient {
                        surfaces
                            .source
                            .push(egui::epaint::ClippedShape { clip_rect, shape: gradient.mesh(rect).into() });
                    }
                }
            });
        }
        if let Some(gradient) = gradient {
            painter.add(gradient.mesh(rect));
        }
        return;
    }
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(rect));
    // Repeated clip edges must not turn a dense arrangement into a grid of raised buttons.
    // Keep the full slider range, but give clips a gentler optical range than large panels.
    let strength = f32::from(level.min(100)) / 100.0 * if surface == Surface::Clip { 0.35 } else { 1.0 };
    if surface == Surface::Panel {
        let backdrop = painter.ctx().data(|data| data.get_temp::<Backdrop>(Id::new(BACKDROP)));
        let Some(backdrop) = backdrop else {
            if let Some(gradient) = gradient {
                painter.add(gradient.mesh(rect));
            }
            return;
        };
        let uv_min = (rect.min - backdrop.screen.min) / backdrop.screen.size();
        let uv_max = (rect.max - backdrop.screen.min) / backdrop.screen.size();
        painter.image(backdrop.texture, rect, Rect::from_min_max(uv_min.to_pos2(), uv_max.to_pos2()), Color32::WHITE);
        painter.add(refraction_mesh(rect, backdrop, strength));
        painter.rect_filled(rect, 0.0, base.gamma_multiply(1.0 - 0.45 * strength));
        painter.add(rim_mesh(rect, strength));
    }
    if surface != Surface::Panel {
        painter.add(body_mesh(rect, base, strength));
        if let Some(gradient) = &gradient {
            painter.add(gradient.mesh(rect));
        }
        let index = painter.add(egui::Shape::Noop);
        painter.ctx().data_mut(|data| {
            let surfaces = data.get_temp_mut_or_default::<Surfaces>(Id::new(SURFACES));
            // Bound CPU geometry even for unusually large visible arrangements.
            if surfaces.lenses.len() < 4096 {
                let clip_rect = painter.clip_rect();
                surfaces
                    .source
                    .push(egui::epaint::ClippedShape { clip_rect, shape: egui::Shape::rect_filled(rect, 0.0, base) });
                surfaces
                    .source
                    .push(egui::epaint::ClippedShape { clip_rect, shape: body_mesh(rect, base, strength).into() });
                if let Some(gradient) = &gradient {
                    surfaces.source.push(egui::epaint::ClippedShape { clip_rect, shape: gradient.mesh(rect).into() });
                }
                surfaces.lenses.push(Lens {
                    layer: painter.layer_id(),
                    clip: painter.clip_rect(),
                    opacity: painter.opacity(),
                    index,
                    rect,
                    strength,
                    mesh: None,
                });
            }
        });
        painter.add(rim_mesh(rect, strength));
        return;
    }
    if let Some(gradient) = gradient {
        painter.add(gradient.mesh(rect));
    }
    let light = motion::mix(base, Color32::WHITE, 0.28 * strength);
    let shade = motion::mix(base, Color32::BLACK, 0.16 * strength);
    let mut mesh = egui::Mesh::default();
    let top = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 22.0_f32.min(rect.height() * 0.34)));
    for (position, color) in [
        (top.left_top(), light.gamma_multiply(0.35 * strength)),
        (top.right_top(), light.gamma_multiply(0.35 * strength)),
        (top.left_bottom(), Color32::TRANSPARENT),
        (top.right_bottom(), Color32::TRANSPARENT),
    ] {
        mesh.colored_vertex(position, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    painter.add(mesh);
    painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, light));
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, shade));
    if surface == Surface::Panel {
        painter.rect_stroke(
            rect,
            0.0,
            theme::hairline(motion::mix(base, theme::text_primary(), 0.12 * strength)),
            StrokeKind::Inside,
        );
    }
}

/// A bounded edge lens over this frame's backdrop. The centre is optically quiet and
/// all coordinates stay inside the captured screen, even against a window edge.
fn refraction_mesh(rect: Rect, backdrop: Backdrop, strength: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(backdrop.sharp_texture.unwrap_or(backdrop.texture));
    if !rect.is_finite()
        || !backdrop.screen.is_finite()
        || !strength.is_finite()
        || rect.width() <= 0.0
        || rect.height() <= 0.0
        || backdrop.screen.width() <= 0.0
        || backdrop.screen.height() <= 0.0
        || strength <= 0.0
    {
        return mesh;
    }
    let strength = strength.min(1.0) * surface_scale(rect);
    if strength == 0.0 {
        return mesh;
    }
    let band = (rect.size().min_elem() * 0.25).min(32.0);
    let axis = |length: f32| {
        [
            0.0,
            band * 0.25,
            band * 0.5,
            band * 0.75,
            band,
            length * 0.5,
            length - band,
            length - band * 0.75,
            length - band * 0.5,
            length - band * 0.25,
            length,
        ]
    };
    let profile = |distance: f32| {
        let t = (distance / band).clamp(0.0, 1.0);
        4.0 * t * (1.0 - t)
    };
    let xs = axis(rect.width());
    let ys = axis(rect.height());
    mesh.vertices.reserve(121);
    mesh.indices.reserve(600);
    for y in ys {
        for x in xs {
            let (left, right, top, bottom) =
                (profile(x), profile(rect.width() - x), profile(y), profile(rect.height() - y));
            let offset = egui::vec2(left - right, top - bottom) * (band * 0.18 * strength);
            let position = rect.min + egui::vec2(x, y);
            let uv = (position + offset - backdrop.screen.min) / backdrop.screen.size();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: position,
                uv: pos2(uv.x.clamp(0.0, 1.0), uv.y.clamp(0.0, 1.0)),
                color: Color32::WHITE.gamma_multiply(left.max(right).max(top).max(bottom) * 0.75 * strength),
            });
        }
    }
    for y in 0..10_u32 {
        for x in 0..10_u32 {
            let i = y * 11 + x;
            mesh.add_triangle(i, i + 1, i + 11);
            mesh.add_triangle(i + 11, i + 1, i + 12);
        }
    }
    mesh
}

/// Broad coloured reflection below the narrow rim and above the source fill.
/// Different left/right intensities give a gentle directional light, not a white stripe.
fn body_mesh(rect: Rect, base: Color32, strength: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    if !rect.is_finite() || rect.width() < 12.0 || rect.height() < 12.0 || !strength.is_finite() {
        return mesh;
    }
    let strength = strength.clamp(0.0, 1.0) * surface_scale(rect);
    if strength == 0.0 {
        return mesh;
    }
    // Fraction of height, left light/shade, right light/shade. Positive reflects light.
    for (fraction, left, right) in
        [(0.0, 0.34_f32, 0.22_f32), (0.25, 0.16, 0.08), (0.55, -0.02, -0.04), (0.86, -0.17, -0.11), (1.0, -0.06, 0.06)]
    {
        let y = rect.top() + rect.height() * fraction;
        for (x, light) in [(rect.left(), left), (rect.right(), right)] {
            let target =
                if light >= 0.0 { Color32::from_white_alpha(base.a()) } else { Color32::from_black_alpha(base.a()) };
            mesh.colored_vertex(pos2(x, y), motion::mix(base, target, light.abs() * strength));
        }
    }
    for row in 0..4_u32 {
        let start = row * 2;
        mesh.add_triangle(start, start + 1, start + 2);
        mesh.add_triangle(start + 2, start + 1, start + 3);
    }
    mesh
}

fn surface_scale(rect: Rect) -> f32 {
    ((rect.height().min(rect.width()) - 12.0) / 28.0).clamp(0.0, 1.0)
}

/// A bounded rounded rim, with a quiet transparent centre. All vertices stay inside
/// the editing rectangle: adjacent clips cannot cast highlights onto one another.
/// This models reflected light only; it never samples or distorts musical content.
fn rim_mesh(rect: Rect, strength: f32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    if !rect.is_finite() || rect.width() < 12.0 || rect.height() < 12.0 || !strength.is_finite() {
        return mesh;
    }
    let scale = surface_scale(rect);
    let strength = strength.clamp(0.0, 1.0) * scale;
    if strength == 0.0 {
        return mesh;
    }
    let radius = (rect.height().min(rect.width()) * 0.16).clamp(2.0, 6.0);
    // Normalised depth, facing highlight, inner shade, opposing light return.
    // The outer transparent ring supplies a subpixel antialiasing fringe.
    let rings = [
        (0.0, 0.0, 0.0, 0.0),
        (0.65, 0.78, 0.06, 0.30),
        (1.25, 0.16, 0.22, 0.04),
        (2.3, 0.0, 0.12, 0.0),
        (3.6, 0.0, 0.0, 0.18),
        (5.5, 0.0, 0.0, 0.0),
    ];
    const CORNER_STEPS: u32 = 8;
    const PERIMETER: u32 = 4 * (CORNER_STEPS + 1);
    mesh.vertices.reserve(rings.len() * PERIMETER as usize);
    mesh.indices.reserve((rings.len() - 1) * PERIMETER as usize * 6);
    for (depth, highlight, shade, returned) in rings {
        let inset = depth * (0.55 + 0.45 * scale);
        let inner = rect.shrink(inset);
        let r = (radius - inset).max(0.0);
        for corner in 0..4 {
            let centre = match corner {
                0 => pos2(inner.right() - r, inner.top() + r),
                1 => pos2(inner.right() - r, inner.bottom() - r),
                2 => pos2(inner.left() + r, inner.bottom() - r),
                _ => pos2(inner.left() + r, inner.top() + r),
            };
            for step in 0..=CORNER_STEPS {
                let angle = (corner as f32 - 1.0 + step as f32 / CORNER_STEPS as f32) * std::f32::consts::FRAC_PI_2;
                let normal = egui::vec2(angle.cos(), angle.sin());
                let facing = normal.dot(egui::vec2(-0.6, -0.8));
                let lit = facing.max(0.0).powi(3);
                let opposite = (-facing).max(0.0).powi(4);
                let white = highlight * (0.12 + 0.88 * lit) + returned * opposite;
                let black = shade * (0.25 + 0.75 * opposite);
                // Each ring has a single premultiplied overlay, preserving the source hue.
                let color = if white >= black {
                    Color32::WHITE.gamma_multiply((white - black) * strength)
                } else {
                    Color32::BLACK.gamma_multiply((black - white) * strength)
                };
                mesh.colored_vertex(centre + normal * r, color);
            }
        }
    }
    for ring in 0..(rings.len() as u32 - 1) {
        for point in 0..PERIMETER {
            let next = (point + 1) % PERIMETER;
            let outer = ring * PERIMETER;
            let inner = outer + PERIMETER;
            mesh.add_triangle(outer + point, outer + next, inner + point);
            mesh.add_triangle(inner + point, outer + next, inner + next);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_that_marks_its_panel_gets_the_window_blurred_under_it() {
        struct Behind;
        impl Provider for Behind {
            fn capture(&self, _: &Context) -> Option<Backdrop> {
                None
            }
            fn capture_behind(
                &self,
                ctx: &Context,
                _shapes: Vec<egui::epaint::ClippedShape>,
                _see_through: bool,
            ) -> Option<Backdrop> {
                Some(Backdrop { texture: TextureId::User(8), sharp_texture: None, screen: ctx.content_rect() })
            }
        }
        let ctx = Context::default();
        install(&ctx, Arc::new(Behind));
        let panel = Color32::from_rgb(4, 5, 6);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let ctx = ui.ctx().clone();
            begin_frame(&ctx, Default::default());
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("dialog")));
            let rect = Rect::from_min_size(pos2(40.0, 40.0), egui::vec2(120.0, 80.0));
            backdrop(&painter, rect);
            painter.rect_filled(rect, 0.0, panel);
            finish_backdrops(&ctx);
        });
        output.textures_delta.clear();
        let shapes: Vec<_> = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
        let image = shapes.iter().position(|s| matches!(s, egui::Shape::Mesh(m) if m.texture_id == TextureId::User(8)));
        let fill = shapes.iter().position(|s| matches!(s, egui::Shape::Rect(r) if r.fill == panel));
        assert!(matches!((image, fill), (Some(image), Some(fill)) if image < fill), "{shapes:?}");
    }

    #[test]
    fn a_menu_gets_the_window_blurred_under_its_frame_and_other_floating_layers_do_not() {
        use std::sync::Mutex;
        struct Behind(Mutex<Vec<egui::epaint::ClippedShape>>);
        impl Provider for Behind {
            fn capture(&self, _: &Context) -> Option<Backdrop> {
                None
            }
            fn capture_behind(
                &self,
                ctx: &Context,
                shapes: Vec<egui::epaint::ClippedShape>,
                _see_through: bool,
            ) -> Option<Backdrop> {
                if let Ok(mut seen) = self.0.lock() {
                    *seen = shapes;
                }
                Some(Backdrop { texture: TextureId::User(7), sharp_texture: None, screen: ctx.content_rect() })
            }
        }
        let host = Arc::new(Behind(Mutex::new(Vec::new())));
        let ctx = Context::default();
        install(&ctx, host.clone());
        let window = Color32::from_rgb(1, 2, 3);
        // A new area is laid out unseen in its first frame; the second shows it.
        let frame = |ui: &mut egui::Ui| {
            let ctx = ui.ctx().clone();
            let area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));
            ctx.layer_painter(egui::LayerId::background()).rect_filled(area, 0.0, window);
            egui::Area::new(Id::new("menu")).order(egui::Order::Foreground).fixed_pos(pos2(20.0, 20.0)).show(
                &ctx,
                |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| ui.label("Open"));
                },
            );
            // A floating layer that is not a popup's frame is left alone.
            egui::Area::new(Id::new("dialog"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(150.0, 20.0))
                .show(&ctx, |ui| ui.label("Dialog"));
            finish_backdrops(&ctx);
        };
        let mut first = ctx.run_ui(Default::default(), frame);
        first.textures_delta.clear();
        let mut output = ctx.run_ui(Default::default(), frame);
        output.textures_delta.clear();
        let shapes: Vec<_> = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
        let image = shapes
            .iter()
            .position(|s| matches!(s, egui::Shape::Mesh(m) if m.texture_id == TextureId::User(7)))
            .expect("the backdrop under the menu");
        let frame = shapes
            .iter()
            .position(|s| match s {
                egui::Shape::Rect(r) => r.fill != window,
                egui::Shape::Vec(parts) => matches!(parts.last(), Some(egui::Shape::Rect(_))),
                _ => false,
            })
            .expect("the menu's frame");
        assert!(image < frame, "the backdrop is drawn before the menu's frame: {shapes:?}");
        let images = shapes.iter().filter(|s| matches!(s, egui::Shape::Mesh(m) if m.texture_id == TextureId::User(7)));
        assert_eq!(images.count(), 1, "only the menu is blurred behind");
        // The capture holds the window's own layers, never the menu.
        let seen = host.0.lock().map(|seen| seen.clone()).unwrap_or_default();
        assert!(seen.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == window)));
        assert!(!seen.iter().any(|c| matches!(&c.shape, egui::Shape::Text(_))));
    }

    #[test]
    fn content_lenses_share_one_current_substrate_and_do_not_poison_panel_capture() {
        use std::sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        };
        struct Observed {
            calls: AtomicUsize,
            sources: Mutex<Vec<Vec<egui::epaint::ClippedShape>>>,
        }
        impl Provider for Observed {
            fn capture(&self, ctx: &Context) -> Option<Backdrop> {
                // A modal replay must never contain the content capture's native texture.
                let order: Vec<_> = ctx.memory(|memory| memory.layer_ids().collect());
                let shapes = ctx.graphics(|layers| layers.clone().drain(&order, &Default::default()));
                assert!(!shapes.iter().any(
                    |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == TextureId::User(99))
                ));
                Some(Backdrop { texture: TextureId::User(100), sharp_texture: None, screen: ctx.content_rect() })
            }
            fn capture_surfaces(&self, ctx: &Context, shapes: Vec<egui::epaint::ClippedShape>) -> Option<Backdrop> {
                self.sources.lock().unwrap().push(shapes);
                self.calls.fetch_add(1, Ordering::SeqCst);
                Some(Backdrop {
                    texture: TextureId::User(99),
                    sharp_texture: Some(TextureId::User(99)),
                    screen: ctx.content_rect(),
                })
            }
        }
        let host = Arc::new(Observed { calls: AtomicUsize::new(0), sources: Mutex::new(Vec::new()) });
        let ctx = Context::default();
        install(&ctx, host.clone());
        configure(&ctx, Options { panels: 50, tracks: 65, clips: 75 });
        for frame in 0..2 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                begin_frame(ui.ctx(), Default::default());
                let painter = ui.painter();
                for (x, surface) in [(20.0, Surface::Track), (240.0, Surface::Clip)] {
                    let rect = Rect::from_min_size(pos2(x, 20.0), egui::vec2(200.0, 80.0));
                    let base = if frame == 0 { Color32::RED } else { Color32::BLUE };
                    painter.rect_filled(rect, 0.0, base);
                    paint(painter, rect, base, surface);
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Sharp notes",
                        egui::FontId::proportional(15.0),
                        Color32::WHITE,
                    );
                }
                finish_surfaces(ui.ctx());
                prepare(ui.ctx());
            });
            assert_eq!(host.calls.load(Ordering::SeqCst), frame + 1);
            assert!(output.shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == TextureId::User(99))
            ));
            output.textures_delta.clear();
        }
        let sources = host.sources.lock().unwrap();
        assert!(sources.iter().flatten().all(|shape| !matches!(shape.shape, egui::Shape::Text(_))));
        assert!(matches!(&sources[0][0].shape, egui::Shape::Rect(rect) if rect.fill == Color32::RED));
        assert!(matches!(&sources[1][0].shape, egui::Shape::Rect(rect) if rect.fill == Color32::BLUE));
    }

    #[test]
    fn coloured_gradient_keeps_the_fill_opacity() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(220.0, 64.0));
        for alpha in [0, 64, 128, 255] {
            let base = Color32::from_rgba_unmultiplied(120, 160, 200, alpha);
            let mesh = body_mesh(rect, base, 1.0);
            assert!(mesh.vertices.iter().all(|v| v.color.a() == alpha));
        }
    }

    #[test]
    fn edge_refraction_bends_current_backdrop_without_moving_the_centre() {
        let screen = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
        let rect = Rect::from_min_size(pos2(50.0, 60.0), egui::vec2(400.0, 300.0));
        let backdrop = Backdrop { screen, texture: TextureId::User(1), sharp_texture: Some(TextureId::User(2)) };
        let mesh = refraction_mesh(rect, backdrop, 1.0);
        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices.len(), 121);
        assert_eq!(mesh.indices.len(), 600);
        assert_eq!(mesh.texture_id, TextureId::User(2));
        assert_eq!(mesh.calc_bounds(), rect);
        let center = &mesh.vertices[60];
        assert_eq!(center.pos, rect.center());
        assert_eq!(center.uv, ((rect.center() - screen.min) / screen.size()).to_pos2());
        assert_eq!(center.color.a(), 0);
        assert!(
            mesh.vertices.iter().any(|v| v.color.a() > 0 && v.uv != ((v.pos - screen.min) / screen.size()).to_pos2())
        );
        for row in mesh.vertices.chunks(11) {
            assert!(row.windows(2).all(|v| v[0].uv.x <= v[1].uv.x), "lens folded its texture");
        }
        for vertex in &mesh.vertices {
            assert!(vertex.uv.x.is_finite() && vertex.uv.y.is_finite());
            assert!((0.0..=1.0).contains(&vertex.uv.x) && (0.0..=1.0).contains(&vertex.uv.y));
        }
        let low = refraction_mesh(rect, backdrop, 0.25);
        assert!(low.vertices.iter().map(|v| v.color.a()).max() < mesh.vertices.iter().map(|v| v.color.a()).max());
        assert!(refraction_mesh(rect, backdrop, f32::NAN).is_empty());
        assert!(refraction_mesh(rect, backdrop, 0.0).is_empty());
        assert!(refraction_mesh(Rect::NOTHING, backdrop, 1.0).is_empty());
        assert!(refraction_mesh(rect, Backdrop { screen: Rect::NOTHING, ..backdrop }, 1.0).is_empty());
        let clipped = refraction_mesh(Rect::from_min_size(pos2(-20.0, -30.0), rect.size()), backdrop, 1.0);
        assert!(clipped.vertices.iter().all(|v| (0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y)));
    }

    #[test]
    fn coloured_body_retains_a_gradient_between_the_rims() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(220.0, 64.0));
        for base in [Color32::from_rgb(204, 210, 175), Color32::from_rgb(45, 60, 80)] {
            let mesh = body_mesh(rect, base, 1.0);
            assert!(mesh.is_valid());
            assert_eq!(mesh.calc_bounds(), rect);
            assert!(mesh.vertices.iter().any(|vertex| vertex.pos.y > rect.top() && vertex.pos.y < rect.bottom()));
            assert!(mesh.vertices.iter().any(|vertex| vertex.color.r() > base.r()));
            assert!(mesh.vertices.iter().any(|vertex| vertex.color.r() < base.r()));
            assert!(mesh.vertices.iter().all(|vertex| vertex.color.is_opaque()));
        }
        assert!(body_mesh(rect, Color32::GRAY, 0.0).is_empty());
        assert!(body_mesh(rect, Color32::GRAY, f32::NAN).is_empty());
        assert!(body_mesh(Rect::NOTHING, Color32::GRAY, 1.0).is_empty());
    }

    #[test]
    fn optical_rim_is_bounded_and_leaves_the_centre_clear() {
        for size in [egui::vec2(13.0, 14.0), egui::vec2(220.0, 64.0), egui::vec2(1600.0, 200.0)] {
            let rect = Rect::from_min_size(pos2(30.0, 50.0), size);
            let mesh = rim_mesh(rect, 1.0);
            assert!(mesh.is_valid());
            assert_eq!(mesh.vertices.len(), 216);
            for vertex in &mesh.vertices {
                assert!(rect.expand(0.001).contains(vertex.pos));
                assert!(vertex.pos.distance(rect.center()) > size.min_elem() * 0.2);
                assert!(vertex.color.r() <= vertex.color.a());
                assert!(vertex.color.g() <= vertex.color.a());
                assert!(vertex.color.b() <= vertex.color.a());
            }
        }
    }

    #[test]
    fn rim_fades_at_small_sizes_and_rejects_invalid_geometry() {
        let area = |height| Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, height));
        let peak = |height, strength| {
            rim_mesh(area(height), strength).vertices.iter().map(|vertex| vertex.color.a()).max().unwrap_or(0)
        };
        assert!(peak(14.0, 1.0) < peak(40.0, 1.0));
        assert!(peak(40.0, 0.5) < peak(40.0, 1.0));
        assert_eq!(peak(12.0, 1.0), 0);
        assert!(rim_mesh(area(40.0), 0.0).is_empty());
        assert!(rim_mesh(area(f32::NAN), 1.0).is_empty());
        assert!(rim_mesh(area(40.0), f32::INFINITY).is_empty());
    }

    #[test]
    fn default_reduced_effects_and_tiny_surfaces_remain_solid() {
        let ctx = Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let ctx = ui.ctx();
            let painter = ctx.layer_painter(egui::LayerId::background());
            let area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 40.0));
            let count =
                || ctx.graphics(|graphics| graphics.get(painter.layer_id()).map_or(0, |list| list.all_entries().len()));
            let before = count();
            paint(&painter, area, Color32::GRAY, Surface::Track);
            assert_eq!(count(), before);
            ctx.data_mut(|data| data.insert_temp(Id::new(OPTIONS), Options { tracks: 50, ..Default::default() }));
            paint(&painter, area, Color32::GRAY, Surface::Track);
            assert!(count() > before);
            let coated = count();
            // The track's slider does not switch on the clip material.
            paint(&painter, area, Color32::GRAY, Surface::Clip);
            assert_eq!(count(), coated);
            set_effects_allowed(ctx, false);
            paint(&painter, area, Color32::GRAY, Surface::Track);
            assert_eq!(count(), coated);
            set_effects_allowed(ctx, true);
            paint(&painter, Rect::from_min_size(area.min, egui::vec2(10.0, 10.0)), Color32::GRAY, Surface::Track);
            assert_eq!(count(), coated);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn failed_capture_does_not_reuse_a_previous_backdrop() {
        let ctx = Context::default();
        ctx.data_mut(|data| {
            data.insert_temp(
                Id::new(BACKDROP),
                Backdrop { texture: TextureId::User(42), sharp_texture: None, screen: Rect::EVERYTHING },
            )
        });
        prepare(&ctx);
        assert!(ctx.data(|data| data.get_temp::<Backdrop>(Id::new(BACKDROP))).is_none());
    }
}
