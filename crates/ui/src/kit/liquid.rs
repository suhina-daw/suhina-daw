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
#[path = "liquid_tests.rs"]
mod tests;
