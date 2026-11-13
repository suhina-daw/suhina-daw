// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opt-in surface materials. The native host supplies the live GPU backdrop; views only
//! describe the surface and paint sharp controls afterward. No audio-thread code lives here.

use std::sync::Arc;

use egui::{Color32, Context, Id, Painter, Rect, Stroke, StrokeKind, TextureId, pos2};

use super::{motion, theme};

/// The theme panel colour before main-window background opacity is applied.
pub fn set_panel_base(ctx: &Context, color: Color32) {
    ctx.data_mut(|data| data.insert_temp(Id::unique("liquid-panel-base"), color));
}

pub fn panel_base(ctx: &Context) -> Color32 {
    ctx.data(|data| data.get_temp::<Color32>(Id::unique("liquid-panel-base"))).unwrap_or_else(theme::window_background)
}

/// A bounded app-owned background image the host may include in its offscreen renderer.
#[derive(Clone)]
pub struct BackgroundImage {
    pub id: TextureId,
    pub pixels: Arc<egui::ColorImage>,
}

pub fn set_background_image(ctx: &Context, image: Option<BackgroundImage>) {
    ctx.data_mut(|data| data.insert_temp(Id::unique("liquid-background-image"), image));
}

pub fn background_image(ctx: &Context) -> Option<BackgroundImage> {
    ctx.data(|data| data.get_temp::<Option<BackgroundImage>>(Id::unique("liquid-background-image"))).flatten()
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
    /// The window's layers and the dialog (or Settings) over them, blurred, for the menus and
    /// tooltips floating over that dialog: targets of its own, apart from [`Provider::capture_behind`]'s,
    /// which the dialog's own backdrop shows in the same frame.
    fn capture_above(
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
        data.insert_temp(Id::unique(SURFACES), Surfaces::default());
        data.insert_temp(Id::unique(GRADIENTS), gradients);
        data.remove_temp::<Vec<Marked>>(Id::unique(MARKED));
    });
    super::floating::begin_frame(ctx);
}

/// Resolve all content lenses once the visible substrate is complete, before modal chrome.
pub fn finish_surfaces(ctx: &Context) {
    let mut surfaces = ctx.data_mut(|data| data.remove_temp::<Surfaces>(Id::unique(SURFACES))).unwrap_or_default();
    if !surfaces.lenses.is_empty() && effects_allowed(ctx) {
        let host = ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST)));
        if let Some(backdrop) = host.and_then(|host| host.0.capture_surfaces(ctx, std::mem::take(&mut surfaces.source)))
        {
            for lens in &mut surfaces.lenses {
                let mesh = refraction_mesh(lens.rect, backdrop, lens.strength);
                let mut painter = lens_painter(ctx, lens);
                painter.set_opacity(lens.opacity);
                painter.set(lens.index, mesh.clone());
                lens.mesh = Some(mesh);
            }
        }
    }
    ctx.data_mut(|data| data.insert_temp(Id::unique(SURFACES), surfaces));
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
    /// The panel's corner radius, which the blur and the cut under it take.
    radius: f32,
    opacity: f32,
}

/// Marks `rect` for the window's content, blurred, under what `painter` paints next: a dialog's
/// panel, called just before its fill. `finish_backdrops` puts the image there once the frame is
/// painted. `radius` is the panel's corner radius.
pub fn backdrop(painter: &Painter, rect: Rect, radius: f32) {
    let ctx = painter.ctx();
    if !effects_allowed(ctx) || !available(ctx) || !rect.is_finite() {
        return;
    }
    let cut = painter.add(egui::Shape::Noop);
    let image = painter.add(egui::Shape::Noop);
    let marked = Marked {
        layer: painter.layer_id(),
        clip: painter.clip_rect(),
        cut,
        image,
        rect,
        radius: radius.clamp(0.0, 128.0),
        opacity: painter.opacity(),
    };
    ctx.data_mut(|data| {
        let all = data.get_temp_mut_or_default::<Vec<Marked>>(Id::unique(MARKED));
        if all.len() < 64 {
            all.push(marked);
        }
    });
}

/// One surface found floating over the window this frame: its layer, where its frame is in it,
/// and what is put under (and in place of) that frame.
struct Floating {
    layer: egui::LayerId,
    index: usize,
    frame: super::floating::Frame,
    shown: f32,
    /// The frame its element paints instead of egui's.
    restyled: Option<super::floating::Restyled>,
    /// Over one of the app's own floating layers (a dialog, Settings): what is behind it is that
    /// layer too, not the window alone.
    over_app: bool,
}

/// The surfaces floating over the window (#1234): every layer at `Order::Foreground` or
/// `Order::Tooltip` the app did not declare its own (egui's menus, context menus, dropdowns and
/// tooltips), and the bar's menus, each with its frame, as far as its element's `backdrop-filter`
/// lets it blur. `order` is the layers from back to front. Also every layer that floats, kept or
/// not, for the captures to leave out.
fn floating(ctx: &Context, order: &[egui::LayerId]) -> (Vec<Floating>, Vec<egui::LayerId>) {
    use super::element::{MENU, POPUP, TOOLTIP};
    use super::floating::Kind;
    let declared = super::floating::declared(ctx);
    let mut below_app = false;
    let wanted: Vec<(egui::LayerId, super::element::Name, bool)> = order
        .iter()
        .filter_map(|layer| {
            let name = match (layer.order, declared.get(&layer.id)) {
                (egui::Order::Foreground, Some(Kind::Own)) => {
                    below_app = true;
                    return None;
                }
                (_, Some(Kind::Own)) => return None,
                (egui::Order::Foreground | egui::Order::Tooltip, Some(Kind::Menu)) => MENU,
                (egui::Order::Foreground, _) => POPUP,
                (egui::Order::Tooltip, _) => TOOLTIP,
                _ => return None,
            };
            Some((*layer, name, below_app))
        })
        .collect();
    let all: Vec<egui::LayerId> = wanted.iter().map(|(layer, _, _)| *layer).collect();
    if wanted.is_empty() {
        return (Vec::new(), all);
    }
    let blurs: Vec<(super::element::Name, bool)> =
        [MENU, POPUP, TOOLTIP].into_iter().map(|name| (name, super::floating::blurs(name))).collect();
    let found: Vec<(egui::LayerId, super::element::Name, bool, usize, super::floating::Frame)> =
        ctx.graphics(|layers| {
            wanted
                .iter()
                .filter_map(|(layer, name, over_app)| {
                    let (index, frame) = super::floating::find_frame(layers.get(*layer)?)?;
                    Some((*layer, *name, *over_app, index, frame))
                })
                .collect()
        });
    let surfaces = found
        .into_iter()
        .filter_map(|(layer, name, over_app, index, frame)| {
            let shown = super::floating::shown(ctx, layer.id);
            if shown <= 0.0 {
                return None;
            }
            // The bar's menus paint `menu` themselves.
            let restyled = if name == MENU { None } else { super::floating::frame(ctx, name, &frame, shown) };
            let blur = blurs.iter().any(|(n, on)| *n == name && *on);
            (blur || restyled.is_some()).then(|| Floating {
                layer,
                index,
                frame,
                shown: if blur { shown } else { 0.0 },
                restyled,
                over_app,
            })
        })
        .collect();
    (surfaces, all)
}

/// The shapes of `layers`, and of no other, as one capture replays them. Only these are copied:
/// `drain` replays every layer it holds, those painted on without an area (a drag's preview) too.
fn shapes_of(ctx: &Context, layers: &[egui::LayerId]) -> Vec<egui::epaint::ClippedShape> {
    let transforms =
        layers.iter().filter_map(|layer| ctx.layer_transform_to_global(*layer).map(|t| (*layer, t))).collect();
    ctx.graphics(|all| {
        let mut copy = egui::layers::GraphicLayers::default();
        for layer in layers {
            if let Some(list) = all.get(*layer) {
                *copy.entry(*layer) = list.clone();
            }
        }
        copy.drain(layers, &transforms)
    })
}

/// What goes where a surface's frame was.
struct Placed {
    layer: egui::LayerId,
    /// The frame's index in its layer.
    index: usize,
    /// The shapes put there: the blurred window, then the frame its element paints, if any.
    shapes: Vec<egui::Shape>,
    /// Whether those replace egui's frame.
    replaced: bool,
    /// What goes over the contents: the frame's outline.
    over: Vec<egui::Shape>,
}

const BLURRED: &str = "liquid-blurred";

/// How many surfaces the last frame put the window, blurred, under: popups, menus, tooltips and
/// dialogs. For the render tool's scenes.
pub fn blurred(ctx: &Context) -> usize {
    ctx.data(|data| data.get_temp::<usize>(Id::unique(BLURRED))).unwrap_or(0)
}

/// Popups, menus, tooltips and the dialogs that asked (`backdrop`) blur what is behind them.
/// The window's own layers are captured once however many surfaces float over them, and not at
/// all when none does; a menu or tooltip over a dialog (or Settings) needs that dialog behind it
/// too, so while one is open a second capture holds the window and the dialog, with the dialog's
/// own blur already in it. Popups' and tooltips' frames are painted from their elements. Called
/// once the frame is painted, before `kit::surface` swaps its backgrounds (the capture cannot
/// replay those).
pub fn finish_backdrops(ctx: &Context) {
    let marked = ctx.data_mut(|data| data.remove_temp::<Vec<Marked>>(Id::unique(MARKED))).unwrap_or_default();
    let order = super::floating::with_window_layers(ctx, ctx.memory(|memory| memory.layer_ids().collect()));
    let (mut surfaces, floating_layers) = floating(ctx, &order);
    let host = ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST))).filter(|_| effects_allowed(ctx));
    if suhina_plugins::trace::on() {
        suhina_plugins::trace::state("menu backdrop", format!("{} floating, {} dialogs", surfaces.len(), marked.len()));
    }
    let see_through = super::surface::available(ctx);
    // In a see-through window the pixels are cleared first, so the sharp content under them does
    // not show through the image's alpha; while a menu or dialog fades in, the image fades in
    // over the content instead. The image takes the frame's corners.
    let under = |backdrop: Backdrop, rect: Rect, radius: egui::CornerRadius, shown: f32| -> Vec<egui::Shape> {
        let uv_min = (rect.min - backdrop.screen.min) / backdrop.screen.size();
        let uv_max = (rect.max - backdrop.screen.min) / backdrop.screen.size();
        let uv = Rect::from_min_max(uv_min.to_pos2(), uv_max.to_pos2());
        let cut = if shown >= 1.0 { super::surface::clear(ctx, rect, radius) } else { None };
        let image = egui::epaint::RectShape::filled(rect, radius, Color32::WHITE.gamma_multiply(shown))
            .with_texture(backdrop.texture, uv);
        cut.into_iter().chain([egui::Shape::Rect(image)]).collect()
    };

    // The window's own layers, for the dialogs and what floats straight over the window.
    let behind = match &host {
        Some(host) if !marked.is_empty() || surfaces.iter().any(|s| s.shown > 0.0 && !s.over_app) => {
            let window: Vec<egui::LayerId> = order
                .iter()
                .copied()
                .filter(|layer| matches!(layer.order, egui::Order::Background | egui::Order::Middle))
                .collect();
            let backdrop = host.0.capture_behind(ctx, shapes_of(ctx, &window), see_through);
            if backdrop.is_none() && suhina_plugins::trace::on() {
                suhina_plugins::trace::state("menu backdrop", "no capture".into());
            }
            backdrop
        }
        _ => None,
    };
    let mut blurred = 0;
    if let Some(backdrop) = behind {
        let dialogs: Vec<(Marked, Vec<egui::Shape>)> = marked
            .into_iter()
            .map(|marked| {
                let shapes = under(
                    backdrop,
                    marked.rect,
                    egui::CornerRadius::from(marked.radius),
                    marked.opacity.clamp(0.0, 1.0),
                );
                (marked, shapes)
            })
            .collect();
        blurred += dialogs.len();
        ctx.graphics_mut(|layers| {
            for (marked, mut shapes) in dialogs {
                let list = layers.entry(marked.layer);
                // Each slot keeps its clip as it is now: a region that hides its overflow may have
                // narrowed it since it was reserved (#1336).
                let clip_of = |list: &egui::layers::PaintList, slot: egui::layers::ShapeIdx| {
                    list.all_entries().nth(slot.0).map_or(marked.clip, |shape| shape.clip_rect)
                };
                let image = shapes.pop().unwrap_or(egui::Shape::Noop);
                if let Some(cut) = shapes.pop() {
                    let clip = clip_of(list, marked.cut);
                    list.set(marked.cut, clip, cut);
                }
                let clip = clip_of(list, marked.image);
                list.set(marked.image, clip, image);
            }
        });
    }

    // The window and the app's own layers over it (dialogs, with their blur now in them), for
    // what floats over a dialog: every layer behind the first such surface but the surfaces.
    let above = match &host {
        Some(host) => surfaces.iter().find(|s| s.shown > 0.0 && s.over_app).and_then(|first| {
            let behind: Vec<egui::LayerId> = order
                .iter()
                .copied()
                .take_while(|layer| *layer != first.layer)
                .filter(|layer| !floating_layers.contains(layer))
                .collect();
            host.0.capture_above(ctx, shapes_of(ctx, &behind), see_through)
        }),
        None => None,
    };
    for surface in &mut surfaces {
        if (if surface.over_app { above } else { behind }).is_none() {
            surface.shown = 0.0;
        }
    }
    surfaces.retain(|surface| surface.shown > 0.0 || surface.restyled.is_some());
    blurred += surfaces.iter().filter(|s| s.shown > 0.0).count();
    ctx.data_mut(|data| data.insert_temp(Id::unique(BLURRED), blurred));
    if surfaces.is_empty() {
        return;
    }
    // What goes where each frame was, worked out before the layers are taken.
    let placed: Vec<Placed> = surfaces
        .into_iter()
        .map(|surface| {
            let radius = surface
                .restyled
                .as_ref()
                .map_or(surface.frame.radius, |restyled| egui::CornerRadius::from(restyled.radius));
            let backdrop = if surface.over_app { above } else { behind };
            let mut shapes = match backdrop {
                Some(backdrop) if surface.shown > 0.0 => under(backdrop, surface.frame.rect, radius, surface.shown),
                _ => Vec::new(),
            };
            let replaced = surface.restyled.is_some();
            let over = match surface.restyled {
                Some(restyled) => {
                    shapes.extend(restyled.under);
                    restyled.over
                }
                None => Vec::new(),
            };
            Placed { layer: surface.layer, index: surface.index, shapes, replaced, over }
        })
        .collect();
    ctx.graphics_mut(|layers| {
        for Placed { layer, index, shapes, replaced, over } in placed {
            let list = layers.entry(layer);
            let entries: Vec<egui::epaint::ClippedShape> = list.all_entries().cloned().collect();
            let mut rebuilt = egui::layers::PaintList::default();
            for (i, clipped) in entries.into_iter().enumerate() {
                if i == index {
                    for shape in &shapes {
                        rebuilt.add(clipped.clip_rect, shape.clone());
                    }
                    if replaced {
                        continue;
                    }
                }
                rebuilt.add(clipped.clip_rect, clipped.shape);
            }
            let clip = rebuilt.all_entries().nth(index).map_or(Rect::EVERYTHING, |shape| shape.clip_rect);
            for shape in over {
                rebuilt.add(clip, shape);
            }
            *list = rebuilt;
        }
    });
}

/// Updated by the app when reduced effects or a high-contrast theme is selected.
pub fn set_effects_allowed(ctx: &Context, allowed: bool) {
    ctx.data_mut(|data| data.insert_temp(Id::unique(ACCESSIBLE), allowed));
}

fn effects_allowed(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(Id::unique(ACCESSIBLE))).unwrap_or(true)
}

/// Install an optional native provider. A host without one keeps solid surfaces.
pub fn install(ctx: &Context, provider: Arc<dyn Provider>) {
    ctx.data_mut(|data| {
        data.insert_temp(Id::unique(HOST), Host(provider));
    });
}

/// Whether the current host supports the experimental material renderer.
pub fn available(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST))).is_some()
}

/// Apply saved per-surface strengths live. Unsupported hosts retain solid surfaces.
pub fn configure(ctx: &Context, options: Options) {
    let options = if available(ctx) { options } else { Options::default() };
    ctx.data_mut(|data| data.insert_temp(Id::unique(OPTIONS), options));
}

/// A painter for `lens`'s reserved slot, clipped as the slot is now: a region that hides its
/// overflow may have narrowed the slot's clip since it was reserved, and setting the slot would
/// otherwise put back the wider one.
fn lens_painter(ctx: &Context, lens: &Lens) -> egui::Painter {
    let clip = ctx
        .graphics(|layers| {
            layers.get(lens.layer).and_then(|list| list.all_entries().nth(lens.index.0)).map(|s| s.clip_rect)
        })
        .unwrap_or(lens.clip);
    ctx.layer_painter(lens.layer).with_clip_rect(clip)
}

/// Forget the previous panel's image, then capture this frame before any modal chrome.
pub fn prepare(ctx: &Context) {
    ctx.data_mut(|data| data.remove::<Backdrop>(Id::unique(BACKDROP)));
    let options = ctx.data(|data| data.get_temp::<Options>(Id::unique(OPTIONS))).unwrap_or_default();
    if options.panels == 0 || !effects_allowed(ctx) {
        return;
    }
    let host = ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST)));
    // Panel capture replays bodies/foreground but excludes native content lenses: the dialog's
    // backdrop never holds glass drawn from another capture.
    let surfaces = ctx.data_mut(|data| data.remove_temp::<Surfaces>(Id::unique(SURFACES))).unwrap_or_default();
    for lens in &surfaces.lenses {
        lens_painter(ctx, lens).set(lens.index, egui::Shape::Noop);
    }
    let backdrop = host.and_then(|host| host.0.capture(ctx));
    for lens in &surfaces.lenses {
        if let Some(mesh) = &lens.mesh {
            let mut painter = lens_painter(ctx, lens);
            painter.set_opacity(lens.opacity);
            painter.set(lens.index, mesh.clone());
        }
    }
    ctx.data_mut(|data| data.insert_temp(Id::unique(SURFACES), surfaces));
    if let Some(backdrop) = backdrop {
        ctx.data_mut(|data| data.insert_temp(Id::unique(BACKDROP), backdrop));
    }
}

/// Paint the base first, then this material, then the surface's text, waveform and controls.
/// Content lenses resolve against a shared foreground-free substrate at finish_surfaces.
/// Floating panels use the complete current UI backdrop. Tiny surfaces stay solid.
/// `el` is the element whose surface this is (a panel, a track's header, a clip): its own CSS
/// background, when the sheet gives one, stands in for the material.
pub fn paint(painter: &Painter, rect: Rect, el: &super::element::Entered, base: Color32, surface: Surface) {
    if !rect.is_finite()
        || rect.width() < 12.0
        || rect.height() < 12.0
        || !painter.clip_rect().intersects(rect)
        || !effects_allowed(painter.ctx())
    {
        return;
    }
    if el.paint_background(painter, rect, 0.0) {
        return;
    }
    let gradient = painter
        .ctx()
        .data(|data| data.get_temp::<super::gradient::Gradients>(Id::unique(GRADIENTS)))
        .and_then(|gradients| match surface {
            Surface::Panel => gradients.panels,
            Surface::Track => gradients.tracks,
            Surface::Clip => gradients.clips,
        });
    let options = painter.ctx().data(|data| data.get_temp::<Options>(Id::unique(OPTIONS))).unwrap_or_default();
    let level = match surface {
        Surface::Panel => options.panels,
        Surface::Track => options.tracks,
        Surface::Clip => options.clips,
    };
    if level == 0 {
        if surface != Surface::Panel && (options.tracks > 0 || options.clips > 0) {
            painter.ctx().data_mut(|data| {
                let surfaces = data.get_temp_mut_or_default::<Surfaces>(Id::unique(SURFACES));
                if surfaces.source.len() < 12288 {
                    let clip_rect = painter.clip_rect().intersect(rect);
                    surfaces
                        .source
                        .push(egui::epaint::ClippedShape::new(clip_rect, egui::Shape::rect_filled(rect, 0.0, base)));
                    if let Some(gradient) = &gradient {
                        surfaces.source.push(egui::epaint::ClippedShape::new(clip_rect, gradient.mesh(rect)));
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
        let backdrop = painter.ctx().data(|data| data.get_temp::<Backdrop>(Id::unique(BACKDROP)));
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
            let surfaces = data.get_temp_mut_or_default::<Surfaces>(Id::unique(SURFACES));
            // Bound CPU geometry even for unusually large visible arrangements.
            if surfaces.lenses.len() < 4096 {
                let clip_rect = painter.clip_rect();
                surfaces
                    .source
                    .push(egui::epaint::ClippedShape::new(clip_rect, egui::Shape::rect_filled(rect, 0.0, base)));
                surfaces.source.push(egui::epaint::ClippedShape::new(clip_rect, body_mesh(rect, base, strength)));
                if let Some(gradient) = &gradient {
                    surfaces.source.push(egui::epaint::ClippedShape::new(clip_rect, gradient.mesh(rect)));
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
