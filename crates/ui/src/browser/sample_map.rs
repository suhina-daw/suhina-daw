// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sample map view (docs/spec/sample-map.md): points laid out by how they sound, coloured by
//! kind, as a cloud or a grid. The view draws and reports what the user did as [`MapEvent`]s;
//! the browser turns them into its own selection, previews and drags.

use std::collections::HashSet;

use egui::{Color32, CornerRadius, Key, PointerButton, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use suhina_soundmap::grid::Grid;
use suhina_soundmap::kinds::Kind;

use crate::browser::sample_map_crowd::{self as crowd, Cluster};
use crate::kit::theme;

/// How long the pointer rests on a point before it is previewed, so a fast sweep does not play
/// everything it crosses.
pub const HOVER_PREVIEW_SECONDS: f64 = 0.040;
/// How long the pointer rests on a point before its tooltip reads the file for a waveform.
pub const HOVER_WAVEFORM_SECONDS: f64 = 0.15;
/// The size of the waveform in a point's tooltip.
const TOOLTIP_WAVE: Vec2 = Vec2::new(220.0, 36.0);
/// How near (in points of the screen) the pointer must be to a point to be over it.
const HIT_RADIUS: f32 = 7.0;
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 64.0;
/// Above this zoom the kind labels fade out, as point names become readable.
const LABEL_FADE_ZOOM: f32 = 6.0;
const TOP_BAR: f32 = 24.0;
const LEGEND: f32 = 20.0;
const SIMILAR_WIDTH: f32 = 180.0;
/// How long the map area must keep one size before the density raster is worked out for it;
/// until then the last raster is drawn scaled (docs/spec/sample-map.md › Many items).
const RESIZE_SETTLE_SECONDS: f64 = 0.15;

/// One point, as the browser gives it to the view.
#[derive(Debug, Clone, PartialEq)]
pub struct MapPoint {
    /// The file name shown in the tooltip and the similar list.
    pub name: String,
    /// More lines for the tooltip: folder, duration, tempo and key.
    pub detail: String,
    /// Position in 0–1 (the cloud layout).
    pub position: [f32; 2],
    pub kind: Kind,
    pub guessed: bool,
    /// The kind was set by hand.
    pub by_hand: bool,
    /// Used by a clip of the active project.
    pub in_use: bool,
    pub selected: bool,
    /// Matches the search text, the tags and the legend (dimmed when not).
    pub matches: bool,
    /// An NKS preset, drawn as a ring.
    pub preset: bool,
    pub pinned: bool,
    /// The colour tags it carries.
    pub tags: Vec<String>,
}

/// Cloud or grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Cloud,
    Grid,
}

/// What the user did on the map. Indices are into the points given to [`show`].
#[derive(Debug, Clone, PartialEq)]
pub enum MapEvent {
    /// Preview this item (the pointer rests on it, or it was clicked or reached by a key).
    Preview(usize),
    /// Select these items; `add` keeps the selection and toggles them.
    Select {
        items: Vec<usize>,
        add: bool,
    },
    /// Drag these items into the project.
    StartDrag(Vec<usize>),
    /// Add this item to the selected track (double-click).
    Add(usize),
    /// The user asked for items similar to this one; the browser answers with
    /// [`MapView::show_similar`].
    Similar(usize),
    /// Pin it, or unpin it when it is pinned.
    Pin(usize),
    /// Set (`on`) or clear a colour tag on it.
    Tag {
        item: usize,
        tag: String,
        on: bool,
    },
    ShowInFolder(usize),
    /// Set its kind by hand, or (`None`) give it back to its name and sound.
    SetKind {
        item: usize,
        kind: Option<Kind>,
    },
    /// The layout switch changed.
    Layout(Layout),
    /// The point size was changed, in percent.
    DotSize(u32),
    /// The preview level was changed, in dB.
    Volume(i32),
    /// The large-view button.
    ToggleLarge,
    /// The button that moves the map into a window of its own, or back into the browser.
    ToggleWindow,
}

/// The view's own state, kept by the browser between frames.
#[derive(Debug, Clone)]
pub struct MapView {
    pub layout: Layout,
    pub preview_on_hover: bool,
    /// The map is shown large, in the main area.
    pub large: bool,
    /// The map is in a window of its own.
    pub window: bool,
    zoom: f32,
    /// The map position at the middle of the view.
    centre: [f32; 2],
    hovered: Option<(usize, f64)>,
    previewed: Option<usize>,
    /// The point the primary button went down on: a drag carries it, not the point under the
    /// pointer once the drag has begun a few pixels away.
    pressed: Option<usize>,
    /// Points are being dragged out of the map: no hover, preview or pan until the drop.
    dragging_items: bool,
    /// The points' size as a share of their size at first (Size in the top bar). Positions do
    /// not change with it, so smaller points stand further apart.
    pub dot_size: f32,
    /// The preview level in dB (Volume in the top bar).
    pub volume: i32,
    /// Kinds the legend shows; the others are dimmed. Empty: all.
    pub legend: HashSet<Kind>,
    /// Shift-drag rectangle, from where it started.
    rubber_band: Option<Pos2>,
    similar: Option<(usize, Vec<usize>)>,
    /// Where the points were drawn last frame.
    area: Rect,
    /// The area the density was last asked for, and since when it has had that size.
    resized: (Rect, f64),
    /// The waveform of the point the pointer rests on, for its tooltip (the panel reads it).
    pub hover_waveform: Option<(usize, std::sync::Arc<crate::kit::waveform::Peaks>)>,
    /// Counts [`MapView::points_changed`], so what is worked out from the points is kept until
    /// they change.
    revision: u64,
    cache: Cache,
}

/// The points the cache was worked out for: [`MapView::points_changed`]'s count, their number and
/// address, the layout and, in the grid, the view's aspect (the grid's positions follow it; the
/// cloud's do not, so resizing the cloud keeps the cache).
type PointsKey = (u64, usize, usize, Layout, u32);

/// What the view works out from the points when they change, not every frame
/// (docs/spec/sample-map.md › Many items).
#[derive(Clone, Default)]
struct Cache {
    /// The points it was worked out for.
    key: Option<PointsKey>,
    /// Items per kind, for the legend, in the order of [`Kind::ALL`].
    counts: Vec<(Kind, usize)>,
    /// The clusters that get a label, largest first.
    clusters: Vec<Cluster>,
    /// The density raster last drawn and what it was drawn for.
    density: Option<Density>,
}

/// A density raster and what it was worked out for.
#[derive(Clone)]
struct Density {
    key: DensityKey,
    /// The area it covers.
    area: Rect,
    /// Where the points were on screen when it was drawn.
    transform: Transform,
    texture: egui::TextureHandle,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache")
            .field("key", &self.key)
            .field("counts", &self.counts)
            .field("clusters", &self.clusters.len())
            .field("density", &self.density.as_ref().map(|d| d.texture.id()))
            .finish()
    }
}

/// Everything the density raster depends on apart from the size of the area it is drawn in; it
/// is drawn again at once when one of these changes. A new size waits for the area to settle
/// ([`density_plan`]).
#[derive(Debug, Clone, PartialEq)]
struct DensityKey {
    points: Option<PointsKey>,
    zoom: f32,
    centre: [f32; 2],
    legend: Vec<Kind>,
    similar: Option<(usize, Vec<usize>)>,
    spread: usize,
}

/// What to do with the density raster this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DensityPlan {
    /// Draw the last one as it is.
    Keep,
    /// Draw the last one scaled into the new area: it is being resized.
    Scale,
    /// Work it out again.
    Build,
}

/// What to do with the density raster `drawn` (what it was worked out for and in which area)
/// when `wanted` is asked for in `area`, which has had its size since `since`. A change of the
/// content builds it at once; a change of the area alone is drawn scaled until the area has
/// kept its size for [`RESIZE_SETTLE_SECONDS`], so a resize does not work it out every frame.
fn density_plan(
    drawn: Option<(&DensityKey, Rect)>,
    wanted: &DensityKey,
    area: Rect,
    since: f64,
    now: f64,
) -> DensityPlan {
    match drawn {
        Some((key, _)) if key != wanted => DensityPlan::Build,
        Some((_, rect)) if rect == area => DensityPlan::Keep,
        Some(_) if now - since < RESIZE_SETTLE_SECONDS => DensityPlan::Scale,
        _ => DensityPlan::Build,
    }
}

impl Default for MapView {
    fn default() -> Self {
        MapView {
            layout: Layout::Cloud,
            preview_on_hover: true,
            large: false,
            window: false,
            zoom: 1.0,
            centre: [0.5, 0.5],
            hovered: None,
            previewed: None,
            pressed: None,
            dragging_items: false,
            dot_size: 1.0,
            volume: crate::settings::PREVIEW_VOLUME,
            legend: HashSet::new(),
            rubber_band: None,
            similar: None,
            area: Rect::NOTHING,
            resized: (Rect::NOTHING, 0.0),
            revision: 0,
            cache: Cache::default(),
            hover_waveform: None,
        }
    }
}

impl MapView {
    /// The points given to [`show`] changed (their positions, kinds or flags): the legend's
    /// counts, the clusters and the density are worked out again.
    pub fn points_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    /// Works out the counts and clusters again when the points changed.
    fn refresh(&mut self, points: &[MapPoint], positions: &[[f32; 2]], aspect: f32) {
        let aspect = if self.layout == Layout::Grid { aspect.to_bits() } else { 0 };
        let key = Some((self.revision, points.len(), points.as_ptr() as usize, self.layout, aspect));
        if self.cache.key == key {
            return;
        }
        let mut counts: Vec<(Kind, usize)> = Kind::ALL.iter().map(|k| (*k, 0)).collect();
        for point in points {
            if let Some((_, n)) = counts.iter_mut().find(|(k, _)| *k == point.kind) {
                *n += 1;
            }
        }
        let items: Vec<([f32; 2], Kind)> =
            points.iter().zip(positions).filter(|(p, _)| p.matches).map(|(p, q)| (*q, p.kind)).collect();
        self.cache = Cache { key, counts, clusters: crowd::clusters(&items), density: None };
    }

    /// The width over the height of where the points were drawn last frame; 1 before the first.
    pub fn aspect(&self) -> f32 {
        aspect_of(self.area)
    }

    /// Shows the answer to [`MapEvent::Similar`]: rings these items and lists them at the side.
    pub fn show_similar(&mut self, of: usize, items: Vec<usize>) {
        self.similar = Some((of, items));
    }

    pub fn similar(&self) -> Option<&(usize, Vec<usize>)> {
        self.similar.as_ref()
    }

    /// The point the pointer has rested on for [`HOVER_WAVEFORM_SECONDS`] by `now`.
    pub fn resting(&self, now: f64) -> Option<usize> {
        self.hovered.filter(|(_, since)| now - since >= HOVER_WAVEFORM_SECONDS).map(|(i, _)| i)
    }

    /// Fits all points in view.
    pub fn fit(&mut self) {
        self.zoom = 1.0;
        self.centre = [0.5, 0.5];
    }

    fn transform(&self, rect: Rect) -> Transform {
        Transform { rect, scale: rect.width().min(rect.height()) * 0.92 * self.zoom, centre: self.centre }
    }

    fn zoom_at(&mut self, rect: Rect, pointer: Pos2, factor: f32) {
        let before = self.transform(rect).to_map(pointer);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.transform(rect).to_map(pointer);
        self.centre = [self.centre[0] + before[0] - after[0], self.centre[1] + before[1] - after[1]];
        self.clamp_centre();
    }

    fn pan(&mut self, rect: Rect, delta: Vec2) {
        let scale = self.transform(rect).scale.max(1e-6);
        self.centre = [self.centre[0] - delta.x / scale, self.centre[1] - delta.y / scale];
        self.clamp_centre();
    }

    fn clamp_centre(&mut self) {
        for c in &mut self.centre {
            *c = c.clamp(0.0, 1.0);
        }
    }
}

/// Map coordinates (0–1) to the screen and back.
#[derive(Debug, Clone, Copy)]
struct Transform {
    rect: Rect,
    scale: f32,
    centre: [f32; 2],
}

impl Transform {
    fn to_screen(self, p: [f32; 2]) -> Pos2 {
        self.rect.center() + Vec2::new((p[0] - self.centre[0]) * self.scale, (p[1] - self.centre[1]) * self.scale)
    }

    fn to_map(self, p: Pos2) -> [f32; 2] {
        let d = p - self.rect.center();
        [self.centre[0] + d.x / self.scale.max(1e-6), self.centre[1] + d.y / self.scale.max(1e-6)]
    }
}

/// Where `rect`, on screen under `from`, is under `to`: the same map positions at `to`'s place and
/// scale, for drawing the last density raster while the area is resized.
fn moved(rect: Rect, from: Transform, to: Transform) -> Rect {
    Rect::from_two_pos(to.to_screen(from.to_map(rect.min)), to.to_screen(from.to_map(rect.max)))
}

/// The width over the height of `rect`; 1 when it has no size.
fn aspect_of(rect: Rect) -> f32 {
    let aspect = rect.width() / rect.height();
    if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 }
}

/// Where each point is drawn: its cloud position, or the centre of its grid cell. The grid
/// fills a box of the view's `aspect` around the map's middle (the view shows 1 across its
/// shorter side), with square cells.
fn positions(points: &[MapPoint], layout: Layout, grid: Option<&Grid>, aspect: f32) -> Vec<[f32; 2]> {
    match (layout, grid) {
        (Layout::Grid, Some(grid)) if grid.cells.len() == points.len() => {
            let (width, height) = if aspect >= 1.0 { (aspect, 1.0) } else { (1.0, 1.0 / aspect) };
            let (columns, rows) = (grid.columns.max(1) as f32, grid.rows.max(1) as f32);
            let cell = (width / columns).min(height / rows);
            grid.cells
                .iter()
                .map(|(c, r)| {
                    [0.5 + (*c as f32 + 0.5 - columns / 2.0) * cell, 0.5 + (*r as f32 + 0.5 - rows / 2.0) * cell]
                })
                .collect()
        }
        _ => points.iter().map(|p| p.position).collect(),
    }
}

/// The next item down (or up) the Show similar list from `from`: the list starts with `of`, the
/// item it was shown for, then the similar ones nearest first. From outside the list, Down goes to
/// the nearest similar one. `None` past either end.
fn step_similar(of: usize, items: &[usize], from: Option<usize>, down: bool) -> Option<usize> {
    let at = from.and_then(|f| std::iter::once(of).chain(items.iter().copied()).position(|i| i == f));
    let list = |n: usize| if n == 0 { Some(of) } else { items.get(n - 1).copied() };
    match (at, down) {
        (Some(at), true) => list(at + 1),
        (Some(at), false) => at.checked_sub(1).and_then(list),
        (None, true) => items.first().copied(),
        (None, false) => None,
    }
}

/// How many times the density kernel is applied for a point size: none for small points, so
/// neighbours stand apart; more for large ones, for soft regions. Once at the size at first.
fn density_spread(dot_size: f32) -> usize {
    match dot_size {
        s if s < 0.75 => 0,
        s if s < 1.5 => 1,
        s if s < 2.5 => 2,
        _ => 3,
    }
}

/// The point nearest to `pointer` on screen, within `radius`.
fn hit(screen: &[Pos2], pointer: Pos2, radius: f32) -> Option<usize> {
    screen
        .iter()
        .enumerate()
        .map(|(i, p)| (i, p.distance_sq(pointer)))
        .filter(|(_, d)| *d <= radius * radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The point nearest to `from` in the direction `direction` (within 60° of it), for the arrow
/// keys.
fn step(positions: &[[f32; 2]], from: usize, direction: [f32; 2]) -> Option<usize> {
    let origin = positions.get(from)?;
    positions
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .filter_map(|(i, p)| {
            let d = [p[0] - origin[0], p[1] - origin[1]];
            let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
            let along = (d[0] * direction[0] + d[1] * direction[1]) / length.max(1e-9);
            // Within 60° of the direction; nearer and straighter is better.
            (length > 0.0 && along >= 0.5).then_some((i, length / along))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A kind's colour: Suhina's muted palette, extended, with neighbouring kinds told apart.
pub fn kind_colour(kind: Kind) -> Color32 {
    match kind {
        Kind::Kick => theme::kind_kick(),
        Kind::Snare => theme::kind_snare(),
        Kind::Clap => theme::kind_clap(),
        Kind::ClosedHat => theme::kind_closed_hat(),
        Kind::OpenHat => theme::kind_open_hat(),
        Kind::Hat => theme::kind_hat(),
        Kind::Rim => theme::kind_rim(),
        Kind::Tom => theme::kind_tom(),
        Kind::Crash => theme::kind_crash(),
        Kind::Ride => theme::kind_ride(),
        Kind::Cymbal => theme::kind_cymbal(),
        Kind::Percussion => theme::kind_percussion(),
        Kind::Bass => theme::kind_bass(),
        Kind::Tonal => theme::kind_tonal(),
        Kind::Vocal => theme::kind_vocal(),
        Kind::Effect => theme::kind_effect(),
        Kind::Other => theme::kind_other(),
    }
}

/// One part of the status's tooltip: a heading ("4 955 could not be read") and lines under it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StatusPart {
    pub heading: String,
    pub lines: Vec<String>,
}

/// Work going on behind the map, shown in a card in the middle of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Busy {
    pub title: String,
    pub detail: String,
    /// From 0 to 1.
    pub fraction: f32,
}

/// Draws the map in the space `ui` offers and reports what the user did.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    view: &mut MapView,
    points: &[MapPoint],
    grid: Option<&Grid>,
    status: &str,
    details: &[StatusPart],
    busy: Option<&Busy>,
    tags: &[String],
    events: &mut Vec<MapEvent>,
) {
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let height = top_bar(ui, full, view, status, details, events);
    let top = Rect::from_min_size(full.min, Vec2::new(full.width(), height));
    let bottom = Rect::from_min_max(Pos2::new(full.min.x, full.max.y - LEGEND), full.max);
    let mut area = Rect::from_min_max(Pos2::new(full.min.x, top.max.y), Pos2::new(full.max.x, bottom.min.y));
    let side = view.similar.is_some().then(|| {
        let side = Rect::from_min_max(Pos2::new(area.max.x - SIMILAR_WIDTH, area.min.y), area.max);
        area.max.x = side.min.x;
        side
    });

    let aspect = aspect_of(area);
    let positions = positions(points, view.layout, grid, aspect);
    view.refresh(points, &positions, aspect);
    map_area(ui, area, view, &Content { points, positions: &positions, grid, tags }, events);
    if let Some(busy) = busy {
        busy_card(ui, area, busy);
    }
    if let Some(side) = side {
        similar_strip(ui, side, view, points, events);
    }
    legend(ui, bottom, view);
}

/// The status's tooltip: the whole status, then each part with its lines.
fn status_tooltip(ui: &mut Ui, status: &str, details: &[StatusPart]) {
    ui.set_max_width(420.0);
    ui.label(egui::RichText::new(status).color(theme::text_primary()));
    for part in details {
        ui.add_space(theme::PAD);
        ui.label(egui::RichText::new(&part.heading).color(theme::text_primary()));
        for line in &part.lines {
            ui.label(egui::RichText::new(line).color(theme::text_secondary()).size(theme::TEXT_SMALL));
        }
    }
}

/// The card in the middle of the map while it is analysed or arranged. It is only painted, so
/// the map under it, the top bar and the rest of the window keep working.
fn busy_card(ui: &Ui, area: Rect, busy: &Busy) {
    const WIDTH: f32 = 240.0;
    const HEIGHT: f32 = 62.0;
    const BAR: f32 = 4.0;
    let width = WIDTH.min(area.width() - 4.0 * theme::PAD);
    if width < 80.0 || area.height() < HEIGHT + 2.0 * theme::PAD {
        return;
    }
    let card = Rect::from_center_size(area.center(), Vec2::new(width, HEIGHT));
    let painter = ui.painter().with_clip_rect(area);
    painter.rect_filled(card, CornerRadius::ZERO, theme::panel_background());
    painter.rect_stroke(card, CornerRadius::ZERO, Stroke::new(1.0, theme::border_strong()), StrokeKind::Inside);
    let inner = card.shrink(2.0 * theme::PAD);
    painter.text(
        inner.left_top(),
        egui::Align2::LEFT_TOP,
        &busy.title,
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let bar = Rect::from_min_size(Pos2::new(inner.min.x, inner.center().y - BAR / 2.0), Vec2::new(inner.width(), BAR));
    painter.rect_filled(bar, CornerRadius::ZERO, theme::control_background());
    let done = Rect::from_min_size(bar.min, Vec2::new(bar.width() * busy.fraction.clamp(0.0, 1.0), BAR));
    painter.rect_filled(done, CornerRadius::ZERO, theme::accent());
    painter.text(
        inner.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        &busy.detail,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// The top bar in the top of `full`: the controls, wrapping onto more rows when the map is
/// narrow, then the status on a row of its own. Returns the bar's height.
fn top_bar(
    ui: &mut Ui,
    full: Rect,
    view: &mut MapView,
    status: &str,
    details: &[StatusPart],
    events: &mut Vec<MapEvent>,
) -> f32 {
    let inner =
        Rect::from_min_max(full.min + Vec2::new(theme::PAD, 2.0), Pos2::new(full.max.x - theme::PAD, full.max.y));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    child.spacing_mut().item_spacing.y = 2.0;
    child.spacing_mut().slider_width = 64.0;
    child.horizontal_wrapped(|ui| controls(ui, view, events));
    // The status is cut short with "…" when it does not fit. Its tooltip has all of it and says
    // why files are missing.
    let text = egui::RichText::new(status).color(theme::text_secondary()).size(theme::TEXT_SMALL);
    child
        .add_sized(
            Vec2::new(inner.width(), theme::TEXT_SMALL + 6.0),
            egui::Label::new(text).truncate().sense(Sense::hover()),
        )
        .on_hover_ui(|ui| status_tooltip(ui, status, details));
    (child.min_rect().bottom() + 2.0 - full.top()).max(TOP_BAR)
}

/// Widgets that stay on one row: when less than `width` is left on the current row, they start
/// the next one.
fn together(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui)) {
    // The room left on this row (a wrapping layout's available width is the whole row's).
    if ui.max_rect().right() - ui.cursor().left() < width {
        ui.end_row();
    }
    add(ui);
}

/// The map's controls, left to right in the order they wrap.
fn controls(ui: &mut Ui, view: &mut MapView, events: &mut Vec<MapEvent>) {
    for (layout, name) in [(Layout::Cloud, "Cloud"), (Layout::Grid, "Grid")] {
        if ui.selectable_label(view.layout == layout, name).clicked() && view.layout != layout {
            view.layout = layout;
            events.push(MapEvent::Layout(layout));
        }
    }
    if ui.button("+").on_hover_text("Zoom in").clicked() {
        view.zoom = (view.zoom * 1.5).min(MAX_ZOOM);
    }
    if ui.button("−").on_hover_text("Zoom out").clicked() {
        view.zoom = (view.zoom / 1.5).max(MIN_ZOOM);
        view.clamp_centre();
    }
    if ui.button("Fit").on_hover_text("Fit all points in view").clicked() {
        view.fit();
    }
    if !view.window
        && ui
            .button(if view.large { "Close" } else { "Large" })
            .on_hover_text("Show the map in the main area")
            .clicked()
    {
        events.push(MapEvent::ToggleLarge);
    }
    if ui
        .button(if view.window { "Dock" } else { "Window" })
        .on_hover_text(if view.window {
            "Put the map back into the browser"
        } else {
            "Open the map in a window of its own, for another screen"
        })
        .clicked()
    {
        events.push(MapEvent::ToggleWindow);
    }
    // Each label stays with its field: the pair moves to the next row as a whole.
    together(ui, 90.0, |ui| {
        ui.label(egui::RichText::new("Size").color(theme::text_secondary()).size(theme::TEXT_SMALL));
        let mut percent = (view.dot_size * 100.0).round() as u32;
        let (low, high) = crate::settings::MAP_DOT_SIZES;
        let size = ui
            .add_sized(
                Vec2::new(48.0, ui.spacing().interact_size.y),
                egui::DragValue::new(&mut percent).range(low..=high).speed(2.0).suffix(" %"),
            )
            .on_hover_text("Point size: drag or type. The points stay where they are, so smaller ones stand apart");
        if size.changed() {
            view.dot_size = percent as f32 / 100.0;
            events.push(MapEvent::DotSize(percent));
        }
    });
    together(ui, 170.0, |ui| {
        ui.label(egui::RichText::new("Volume").color(theme::text_secondary()).size(theme::TEXT_SMALL));
        let (low, high) = crate::settings::PREVIEW_VOLUMES;
        let mut db = view.volume;
        let volume = ui
            .add(egui::Slider::new(&mut db, low..=high).suffix(" dB").step_by(1.0))
            .on_hover_text("The level samples are previewed at; it also changes the one playing");
        if volume.changed() && db != view.volume {
            view.volume = db;
            events.push(MapEvent::Volume(db));
        }
    });
}

/// What the map area draws.
struct Content<'a> {
    points: &'a [MapPoint],
    /// Where each point is, in map coordinates.
    positions: &'a [[f32; 2]],
    grid: Option<&'a Grid>,
    /// The library's colour tags, for the right-click menu.
    tags: &'a [String],
}

fn map_area(ui: &mut Ui, rect: Rect, view: &mut MapView, content: &Content<'_>, events: &mut Vec<MapEvent>) {
    let Content { points, positions, grid, tags } = *content;
    view.area = rect;
    let id = ui.id().with("sample-map");
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, theme::window_background());

    // Zoom and pan.
    if let Some(pointer) = response.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let pinch = ui.input(|i| i.zoom_delta());
        let factor = (scroll / 200.0).exp() * pinch;
        if (factor - 1.0).abs() > 1e-4 {
            view.zoom_at(rect, pointer, factor);
        }
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let ctrl = ui.input(|i| i.modifiers.command);
    let transform = view.transform(rect);
    let screen: Vec<Pos2> = positions.iter().map(|p| transform.to_screen(*p)).collect();
    let pointer = response.hover_pos();
    let under = pointer.and_then(|p| hit(&screen, p, HIT_RADIUS.max(cell_size(view, grid, transform) / 2.0)));

    let primary_pressed = response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed());
    if primary_pressed {
        view.pressed = under;
    }
    if response.drag_started_by(PointerButton::Primary) {
        // Where the button went down, not where the pointer is now: the drag only begins once it
        // has moved, and in a dense cloud another point is nearest by then.
        let origin = ui.input(|i| i.pointer.press_origin()).or_else(|| response.interact_pointer_pos());
        let on = view.pressed.take().or_else(|| origin.and_then(|p| hit(&screen, p, HIT_RADIUS)));
        match on {
            Some(i) => {
                let selected: Vec<usize> =
                    points.iter().enumerate().filter(|(_, p)| p.selected).map(|(j, _)| j).collect();
                let items = if points.get(i).is_some_and(|p| p.selected) { selected } else { vec![i] };
                events.push(MapEvent::StartDrag(items));
                view.dragging_items = true;
                view.hovered = None;
            }
            None if shift => view.rubber_band = origin,
            None => {}
        }
    }
    if !response.dragged() && !response.drag_started() {
        view.dragging_items = false;
    }
    if response.dragged_by(PointerButton::Primary)
        && view.rubber_band.is_none()
        && !view.dragging_items
        && view.pressed.is_none()
        || response.dragged_by(PointerButton::Middle)
    {
        view.pan(rect, response.drag_delta());
    }
    if let Some(start) = view.rubber_band {
        let end = pointer.or_else(|| response.interact_pointer_pos()).unwrap_or(start);
        let band = Rect::from_two_pos(start, end);
        painter.rect_stroke(band, CornerRadius::ZERO, Stroke::new(1.0, theme::accent()), StrokeKind::Inside);
        if response.drag_stopped() {
            let items: Vec<usize> =
                screen.iter().enumerate().filter(|(_, p)| band.contains(**p)).map(|(i, _)| i).collect();
            events.push(MapEvent::Select { items, add: true });
            view.rubber_band = None;
        }
    }

    // Clicks.
    if response.double_clicked() {
        match under {
            Some(i) => events.push(MapEvent::Add(i)),
            None => view.fit(),
        }
    } else {
        // A point is selected and previewed when the button goes down, so it sounds at once. A
        // point of a selection of several waits for the release, so pressing it to drag them
        // all keeps the selection.
        let many = points.iter().filter(|p| p.selected).count() > 1;
        let deferred = |i: usize| many && !ctrl && points.get(i).is_some_and(|p| p.selected);
        if primary_pressed && let Some(i) = under.filter(|i| !deferred(*i)) {
            events.push(MapEvent::Select { items: vec![i], add: ctrl });
            events.push(MapEvent::Preview(i));
            view.previewed = Some(i);
        }
        if response.clicked() {
            match under {
                Some(i) if deferred(i) => {
                    events.push(MapEvent::Select { items: vec![i], add: false });
                    events.push(MapEvent::Preview(i));
                    view.previewed = Some(i);
                }
                Some(_) => {}
                None => view.similar = None,
            }
        }
    }
    if response.secondary_clicked() {
        view.hovered = under.map(|i| (i, 0.0));
    }
    let menu_item = view.hovered.map(|(i, _)| i).or(under);
    response.context_menu(|ui| {
        if let Some(i) = menu_item {
            if ui.button("Show similar").clicked() {
                events.push(MapEvent::Similar(i));
                ui.close();
            }
            let pinned = points.get(i).is_some_and(|p| p.pinned);
            if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                events.push(MapEvent::Pin(i));
                ui.close();
            }
            if !tags.is_empty() {
                ui.menu_button("Tag", |ui| {
                    for tag in tags {
                        let mut on = points.get(i).is_some_and(|p| p.tags.contains(tag));
                        if ui.checkbox(&mut on, tag.as_str()).clicked() {
                            events.push(MapEvent::Tag { item: i, tag: tag.clone(), on });
                            ui.close();
                        }
                    }
                });
            }
            if let Some(point) = points.get(i) {
                ui.menu_button("Kind", |ui| kind_menu(ui, i, point, events));
            }
            if ui.button("Show in folder").clicked() {
                events.push(MapEvent::ShowInFolder(i));
                ui.close();
            }
        }
    });

    // Preview on hover, after the pointer rests; not while points are dragged out of the map.
    let now = ui.input(|i| i.time);
    let under = under.filter(|_| !view.dragging_items);
    match (under, view.hovered) {
        (Some(i), Some((j, since))) if i == j => {
            if view.preview_on_hover && view.previewed != Some(i) && now - since >= HOVER_PREVIEW_SECONDS {
                events.push(MapEvent::Preview(i));
                view.previewed = Some(i);
            } else if view.previewed != Some(i) {
                ui.ctx().request_repaint();
            }
        }
        (Some(i), _) => {
            view.hovered = Some((i, now));
            ui.ctx().request_repaint();
        }
        (None, _) if !response.context_menu_opened() => {
            view.hovered = None;
            if !view.preview_on_hover {
                view.previewed = None;
            }
        }
        (None, _) => {}
    }

    // Keys: arrows step to the nearest point that way, or with Show similar on, Up and Down step
    // through its list; Escape ends Show similar.
    if response.contains_pointer() || response.has_focus() {
        let keys = [
            (Key::ArrowLeft, [-1.0, 0.0]),
            (Key::ArrowRight, [1.0, 0.0]),
            (Key::ArrowUp, [0.0, -1.0]),
            (Key::ArrowDown, [0.0, 1.0]),
        ];
        for (key, direction) in keys {
            if ui.input(|i| i.key_pressed(key)) {
                let from = points.iter().position(|p| p.selected).or(view.previewed);
                let vertical = matches!(key, Key::ArrowUp | Key::ArrowDown);
                let next = match (view.similar.as_ref().filter(|_| vertical), from) {
                    (Some((of, items)), from) => step_similar(*of, items, from, key == Key::ArrowDown),
                    (None, Some(from)) => step(positions, from, direction),
                    (None, None) => points.iter().position(|p| p.matches),
                };
                if let Some(next) = next {
                    events.push(MapEvent::Select { items: vec![next], add: false });
                    events.push(MapEvent::Preview(next));
                    view.previewed = Some(next);
                }
            }
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            // Escape ends Show similar first, then closes the large map.
            if view.similar.take().is_none() && view.large {
                events.push(MapEvent::ToggleLarge);
            }
        }
    }

    draw_points(&painter, view, points, &screen, grid, transform, under);
    draw_labels(&painter, view, transform);

    if let Some(point) = under.and_then(|i| points.get(i)) {
        let kind = kind_text(point);
        response.on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(&point.name).color(theme::text_primary()));
            ui.label(egui::RichText::new(kind).color(theme::text_secondary()));
            if !point.detail.is_empty() {
                ui.label(egui::RichText::new(&point.detail).color(theme::text_secondary()));
            }
            if let Some((_, peaks)) = view.hover_waveform.as_ref().filter(|(i, _)| Some(*i) == under) {
                let (rect, _) = ui.allocate_exact_size(TOOLTIP_WAVE, Sense::hover());
                let seconds = peaks.frames as f64 / f64::from(peaks.sample_rate.max(1));
                let count = rect.width() as usize;
                let columns =
                    crate::kit::waveform::steady_columns(rect.left(), count, 0.0, seconds / count.max(1) as f64, 1.0);
                let shape =
                    crate::kit::waveform::geometry(peaks, rect, &columns, crate::kit::waveform::scale(ui.ctx()));
                crate::kit::waveform::paint(
                    ui.painter(),
                    &shape,
                    crate::kit::waveform::Colors::from(theme::text_secondary()),
                );
            }
        });
    }
}

/// The side of a grid cell on screen (0 in the cloud).
/// A point's kind as its tooltip says it.
fn kind_text(point: &MapPoint) -> String {
    match (point.by_hand, point.guessed) {
        (true, _) => format!("{} (set by hand)", point.kind.label()),
        (false, true) => format!("{} (guessed)", point.kind.label()),
        (false, false) => point.kind.label().to_string(),
    }
}

/// The point menu's **Kind** submenu: **Automatic** (the name and the sound decide), then every
/// kind, the one set by hand marked.
fn kind_menu(ui: &mut Ui, item: usize, point: &MapPoint, events: &mut Vec<MapEvent>) {
    if ui.radio(!point.by_hand, "Automatic").on_hover_text("From its name and its sound").clicked() {
        events.push(MapEvent::SetKind { item, kind: None });
        ui.close();
    }
    ui.separator();
    for kind in Kind::ALL {
        if ui.radio(point.by_hand && point.kind == kind, kind.label()).clicked() {
            events.push(MapEvent::SetKind { item, kind: Some(kind) });
            ui.close();
        }
    }
}

fn cell_size(view: &MapView, grid: Option<&Grid>, transform: Transform) -> f32 {
    match (view.layout, grid) {
        (Layout::Grid, Some(grid)) => transform.scale / grid.columns.max(grid.rows).max(1) as f32,
        _ => 0.0,
    }
}

/// The points: one dot each, or the density raster with the points that must be seen one by one
/// drawn on top when too many are in view (docs/spec/sample-map.md › Many items).
fn draw_points(
    painter: &egui::Painter,
    view: &mut MapView,
    points: &[MapPoint],
    screen: &[Pos2],
    grid: Option<&Grid>,
    transform: Transform,
    under: Option<usize>,
) {
    let area = painter.clip_rect();
    let visible = area.expand(8.0);
    let cell = cell_size(view, grid, transform);
    let in_view = screen.iter().filter(|p| area.contains(**p)).count();
    let crowded = if cell > 0.0 { cell < crowd::BIN } else { in_view > crowd::DENSITY_ABOVE };
    let legend = view.legend.clone();
    let similar_of = view.similar.as_ref().map(|(of, _)| *of);
    let similar: HashSet<usize> = view.similar.as_ref().map(|(_, s)| s.iter().copied().collect()).unwrap_or_default();
    let dimmed = |i: usize, p: &MapPoint| {
        !p.matches
            || !legend.is_empty() && !legend.contains(&p.kind)
            || similar_of.is_some_and(|of| of != i) && !similar.contains(&i)
    };
    if crowded {
        let mut kinds: Vec<Kind> = legend.iter().copied().collect();
        kinds.sort();
        let key = DensityKey {
            points: view.cache.key,
            zoom: view.zoom,
            centre: view.centre,
            legend: kinds,
            similar: view.similar.clone(),
            spread: density_spread(view.dot_size),
        };
        let now = painter.ctx().input(|i| i.time);
        if view.resized.0 != area {
            view.resized = (area, now);
        }
        let drawn = view.cache.density.as_ref().map(|d| (&d.key, d.area));
        match density_plan(drawn, &key, area, view.resized.1, now) {
            DensityPlan::Build => {
                let colour = |i: usize| points.get(i).filter(|p| !dimmed(i, p)).map(|p| kind_colour(p.kind));
                let image = crowd::density(screen, colour, area, key.spread);
                let texture = painter.ctx().load_texture("sample-map-density", image, egui::TextureOptions::LINEAR);
                view.cache.density = Some(Density { key, area, transform, texture });
            }
            DensityPlan::Scale => {
                // Come back once the size has settled, even if nothing else asks for a frame.
                let left = RESIZE_SETTLE_SECONDS - (now - view.resized.1);
                painter.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.0)));
            }
            DensityPlan::Keep => {}
        }
        if let Some(density) = view.cache.density.as_ref() {
            let drawn = Rect::from_min_size(density.area.min, density.texture.size_vec2() * crowd::BIN);
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(density.texture.id(), moved(drawn, density.transform, transform), uv, Color32::WHITE);
        }
    }
    let (radius, opacity) = if crowded { (3.0, 1.0) } else { crowd::dot_style(in_view, area.area()) };
    let radius = (radius * view.dot_size).clamp(0.75, 24.0);
    let cell = if crowded { 0.0 } else { cell };
    // Dimmed points first, so the others are drawn over them.
    for pass in [true, false] {
        for (i, (point, at)) in points.iter().zip(screen).enumerate() {
            if dimmed(i, point) != pass || !visible.contains(*at) {
                continue;
            }
            let marked =
                point.selected || point.in_use || similar.contains(&i) || view.previewed == Some(i) || under == Some(i);
            // In the density view only the points that must be seen one by one are drawn.
            if crowded && !marked {
                continue;
            }
            let mut colour = kind_colour(point.kind);
            if point.guessed {
                colour = colour.gamma_multiply(0.7);
            }
            if pass {
                colour = colour.gamma_multiply(0.18);
            } else if !marked {
                colour = colour.gamma_multiply(opacity);
            }
            if cell > 0.0 {
                let side = (cell - 1.0).max(1.0);
                let r = Rect::from_center_size(*at, Vec2::splat(side));
                if point.preset {
                    painter.rect_stroke(
                        r.shrink(1.0),
                        CornerRadius::same(2),
                        Stroke::new(1.5, colour),
                        StrokeKind::Inside,
                    );
                } else {
                    painter.rect_filled(r, CornerRadius::same(2), colour);
                }
            } else if point.preset {
                painter.circle_stroke(*at, radius + 0.5, Stroke::new(1.2, colour));
            } else {
                painter.circle_filled(*at, radius, colour);
            }
            let size = if cell > 0.0 { cell / 2.0 } else { radius };
            if point.in_use {
                painter.circle_stroke(*at, size + 3.0, Stroke::new(1.5, colour));
            }
            if point.selected {
                painter.circle_stroke(*at, size + 1.5, Stroke::new(1.0, theme::text_primary()));
            }
            if similar.contains(&i) {
                painter.circle_stroke(*at, size + 4.5, Stroke::new(1.0, theme::accent()));
            }
            if view.previewed == Some(i) || under == Some(i) {
                painter.circle_stroke(*at, size + 2.5, Stroke::new(1.5, theme::accent()));
            }
        }
    }
}

/// One label per cluster, sized by its count, placed largest first so none overlap
/// (docs/spec/sample-map.md › Many items › Labels).
fn draw_labels(painter: &egui::Painter, view: &MapView, transform: Transform) {
    let fade = ((LABEL_FADE_ZOOM - view.zoom) / 2.0).clamp(0.0, 1.0);
    if fade <= 0.0 {
        return;
    }
    let clusters: Vec<&Cluster> =
        view.cache.clusters.iter().filter(|c| view.legend.is_empty() || view.legend.contains(&c.kind)).collect();
    let smallest = clusters.last().map_or(1, |c| c.count);
    let galleys: Vec<_> = clusters
        .iter()
        .map(|c| {
            let font = theme::ui_font(theme::TEXT * 1.6 * crowd::label_scale(c.count, smallest));
            // Light text: a label sits on its own kind's colour, so that colour would not read.
            painter.layout_no_wrap(c.kind.label().to_string(), font, theme::text_primary().gamma_multiply(fade))
        })
        .collect();
    let sizes: Vec<(Pos2, Vec2)> =
        clusters.iter().zip(&galleys).map(|(c, g)| (transform.to_screen(c.spot), g.size())).collect();
    let shadow = theme::window_background().gamma_multiply(fade);
    for (spot, galley) in crowd::place_labels(&sizes, painter.clip_rect()).into_iter().zip(galleys) {
        let Some(spot) = spot else { continue };
        // A shadow keeps the label readable over the points.
        painter.galley_with_override_text_color(spot.min + Vec2::new(1.0, 1.0), galley.clone(), shadow);
        painter.galley(spot.min, galley, shadow);
    }
}

fn similar_strip(ui: &mut Ui, rect: Rect, view: &mut MapView, points: &[MapPoint], events: &mut Vec<MapEvent>) {
    let Some((of, items)) = view.similar.clone() else { return };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, theme::panel_background());
    let mut child = ui.new_child(
        egui::UiBuilder::new().max_rect(rect.shrink(theme::PAD)).layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let name = points.get(of).map_or("", |p| p.name.as_str());
    child.label(
        egui::RichText::new(format!("Similar to {name}")).color(theme::text_secondary()).size(theme::TEXT_SMALL),
    );
    egui::ScrollArea::vertical().show(&mut child, |ui| {
        for i in items {
            let Some(point) = points.get(i) else { continue };
            ui.horizontal(|ui| {
                if ui.small_button("▶").on_hover_text(format!("Preview {}", point.name)).clicked() {
                    events.push(MapEvent::Preview(i));
                }
                let row = ui
                    .selectable_label(point.selected, egui::RichText::new(&point.name).color(kind_colour(point.kind)));
                if row.clicked() {
                    events.push(MapEvent::Select { items: vec![i], add: false });
                    events.push(MapEvent::Preview(i));
                }
            });
        }
    });
    if child.button("Close").clicked() {
        view.similar = None;
    }
}

fn legend(ui: &mut Ui, rect: Rect, view: &mut MapView) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(theme::PAD, 1.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ctrl = child.input(|i| i.modifiers.command);
    for (kind, count) in view.cache.counts.clone() {
        if count == 0 {
            continue;
        }
        let shown = view.legend.is_empty() || view.legend.contains(&kind);
        let colour = if shown { kind_colour(kind) } else { kind_colour(kind).gamma_multiply(0.3) };
        let (dot, dot_response) = child.allocate_exact_size(Vec2::splat(10.0), Sense::click());
        child.painter().circle_filled(dot.center(), 3.5, colour);
        let text = egui::RichText::new(format!("{} {count}", kind.label())).color(colour).size(theme::TEXT_SMALL);
        let response: Response = child.add(egui::Label::new(text).sense(Sense::click()));
        child.add_space(theme::PAD);
        if response.clicked() || dot_response.clicked() {
            toggle_legend(&mut view.legend, kind, ctrl);
        }
    }
}

/// A click on a kind in the legend: show only it (Ctrl: add or remove it); clicking the only
/// kind shown shows all again.
fn toggle_legend(legend: &mut HashSet<Kind>, kind: Kind, add: bool) {
    if add {
        if !legend.remove(&kind) {
            legend.insert(kind);
        }
    } else if legend.len() == 1 && legend.contains(&kind) {
        legend.clear();
    } else {
        legend.clear();
        legend.insert(kind);
    }
}

#[cfg(test)]
#[path = "sample_map_tests.rs"]
mod tests;

#[cfg(test)]
mod many {
    use super::*;

    /// `n` points in clusters of the 17 kinds, from a fixed seed.
    pub(super) fn points(n: usize) -> Vec<MapPoint> {
        let mut state = 7u32;
        let mut noise = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32
        };
        (0..n)
            .map(|i| {
                let k = i % Kind::ALL.len();
                let kind = Kind::ALL[k];
                let centre = [0.15 + (k % 5) as f32 * 0.18, 0.15 + (k / 5) as f32 * 0.22];
                MapPoint {
                    name: format!("{} {i}", kind.label()),
                    detail: String::new(),
                    // Roughly Gaussian clouds (the sum of three uniform draws), as t-SNE makes them.
                    position: [
                        (centre[0] + (noise() + noise() + noise() - 1.5) * 0.06).clamp(0.0, 1.0),
                        (centre[1] + (noise() + noise() + noise() - 1.5) * 0.07).clamp(0.0, 1.0),
                    ],
                    kind,
                    guessed: false,
                    by_hand: false,
                    in_use: i % 5_000 == 0,
                    selected: i == 3,
                    matches: true,
                    preset: false,
                    pinned: false,
                    tags: Vec::new(),
                }
            })
            .collect()
    }

    /// The slowest of `frames` frames (the view's work and tessellation) with the pointer moving
    /// over the map and, when `pan`, the map moved every frame so the density is drawn anew.
    fn slowest_frame(points: &[MapPoint], frames: usize, pan: bool) -> std::time::Duration {
        let ctx = egui::Context::default();
        let mut view = MapView::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let mut slowest = std::time::Duration::ZERO;
        for frame in 0..frames + 2 {
            let mut input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            input.events.push(egui::Event::PointerMoved(Pos2::new(300.0 + frame as f32 * 7.0, 400.0)));
            if pan {
                view.centre = [0.5 + frame as f32 * 0.001, 0.5];
            }
            let started = std::time::Instant::now();
            let mut output = ctx.run_ui(input, |ui| {
                let mut events = Vec::new();
                show(ui, &mut view, points, None, "200 000 samples", &[], None, &[], &mut events);
            });
            let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
            // No renderer here takes the density texture.
            output.textures_delta.clear();
            // The first frames lay out the fonts and work out the clusters.
            if frame >= 2 {
                slowest = slowest.max(started.elapsed());
            }
        }
        slowest
    }

    /// Resizes a 200 000-point map every frame for `frames` frames of 1/60 s, then leaves it
    /// alone for `settle` frames. Returns the slowest resize frame and how many density rasters
    /// were worked out after the first frame.
    fn resize(points: &[MapPoint], frames: usize, settle: usize) -> (std::time::Duration, usize) {
        let ctx = egui::Context::default();
        let mut view = MapView::default();
        let mut slowest = std::time::Duration::ZERO;
        let mut textures = Vec::new();
        for frame in 0..1 + frames + settle {
            let grow = frame.min(frames) as f32 * 6.0;
            let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0 + grow, 700.0 + grow * 0.5));
            let input =
                egui::RawInput { screen_rect: Some(screen), time: Some(frame as f64 / 60.0), ..Default::default() };
            let started = std::time::Instant::now();
            let mut output = ctx.run_ui(input, |ui| {
                let mut events = Vec::new();
                show(ui, &mut view, points, None, "200 000 samples", &[], None, &[], &mut events);
            });
            let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
            output.textures_delta.clear();
            if (1..=frames).contains(&frame) {
                slowest = slowest.max(started.elapsed());
            }
            if let Some(density) = view.cache.density.as_ref()
                && textures.last() != Some(&density.texture.id())
            {
                textures.push(density.texture.id());
            }
        }
        (slowest, textures.len().saturating_sub(1))
    }

    #[test]
    fn resizing_a_map_of_200_000_points_does_not_draw_the_density_every_frame() {
        let points = points(200_000);
        let (slowest, built) = resize(&points, 30, 0);
        assert_eq!(built, 0, "while the size keeps changing, the last raster is drawn scaled");
        let (_, built) = resize(&points, 30, 15);
        assert_eq!(built, 1, "once the size has settled, it is worked out once for it");
        eprintln!("200 000 points: slowest resize frame {slowest:?}");
        // 30 frames a second in an optimised build; unoptimised test builds get more room.
        let budget = std::time::Duration::from_micros(if cfg!(debug_assertions) { 100_000 } else { 33_300 });
        assert!(slowest < budget, "slowest resize frame {slowest:?}");
    }

    #[test]
    fn a_map_of_200_000_points_draws_at_60_frames_a_second() {
        let points = points(200_000);
        let still = slowest_frame(&points, 10, false);
        let panning = slowest_frame(&points, 10, true);
        eprintln!("200 000 points: still {still:?}, panning {panning:?}");
        // A frame at 60 Hz in an optimised build; unoptimised test builds get more room.
        let budget = std::time::Duration::from_micros(if cfg!(debug_assertions) { 100_000 } else { 16_700 });
        assert!(still < budget && panning < budget, "still {still:?}, panning {panning:?}");
    }
}

#[cfg(test)]
mod similar_keys {
    use super::*;

    #[test]
    fn up_and_down_step_through_the_similar_list() {
        let items = [7, 3, 12];
        assert_eq!(step_similar(0, &items, None, true), Some(7));
        assert_eq!(step_similar(0, &items, Some(0), true), Some(7));
        assert_eq!(step_similar(0, &items, Some(7), true), Some(3));
        assert_eq!(step_similar(0, &items, Some(12), true), None, "past the end");
        assert_eq!(step_similar(0, &items, Some(3), false), Some(7));
        assert_eq!(step_similar(0, &items, Some(7), false), Some(0), "back to the item it was shown for");
        assert_eq!(step_similar(0, &items, Some(0), false), None);
        assert_eq!(step_similar(0, &items, Some(5), true), Some(7), "from outside the list");
    }

    #[test]
    fn down_with_show_similar_on_previews_the_next_similar_item() {
        use egui_kittest::Harness;
        let point = |x: f32, y: f32| MapPoint {
            name: format!("{x},{y}"),
            detail: String::new(),
            position: [x, y],
            kind: Kind::Kick,
            guessed: false,
            by_hand: false,
            in_use: false,
            selected: false,
            matches: true,
            preset: false,
            pinned: false,
            tags: Vec::new(),
        };
        // A row of points left to right: spatially, Down from point 0 has nowhere to go.
        let points: Vec<MapPoint> = (0..6).map(|i| point(i as f32 / 5.0, 0.5)).collect();
        let mut view = MapView::default();
        view.show_similar(0, vec![4, 2]);
        let mut harness = Harness::builder().with_size(egui::vec2(500.0, 340.0)).build_ui_state(
            |ui, (view, events): &mut (MapView, Vec<MapEvent>)| {
                show(ui, view, &points, None, "6 samples", &[], None, &[], events)
            },
            (view, Vec::<MapEvent>::new()),
        );
        harness.run();
        let view = &harness.state().0;
        let middle = view.area.center();
        harness.hover_at(middle);
        harness.run();
        harness.state_mut().0.previewed = Some(0);
        harness.state_mut().1.clear();
        harness.key_press(Key::ArrowDown);
        harness.run();
        assert!(harness.state().1.contains(&MapEvent::Preview(4)), "{:?}", harness.state().1);
        harness.state_mut().1.clear();
        harness.key_press(Key::ArrowDown);
        harness.run();
        assert!(harness.state().1.contains(&MapEvent::Preview(2)), "{:?}", harness.state().1);
        harness.state_mut().1.clear();
        // Right still steps on the map: from point 2 to its neighbour 3.
        harness.key_press(Key::ArrowRight);
        harness.run();
        assert!(harness.state().1.contains(&MapEvent::Preview(3)), "{:?}", harness.state().1);
    }
}

#[cfg(test)]
mod look {
    use super::*;

    /// Renders a map of generated clusters to `target/sample-map.png` to look at (not a test of
    /// behaviour; run with `--ignored`).
    #[test]
    #[ignore]
    fn render_a_map() {
        use egui_kittest::Harness;
        let mut state = 5u32;
        let mut noise = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as f32 / u32::MAX as f32 - 0.5
        };
        let centres = [
            (Kind::Kick, [0.2, 0.6]),
            (Kind::Bass, [0.3, 0.8]),
            (Kind::Snare, [0.5, 0.4]),
            (Kind::Clap, [0.6, 0.55]),
            (Kind::ClosedHat, [0.8, 0.3]),
            (Kind::OpenHat, [0.85, 0.5]),
            (Kind::Percussion, [0.5, 0.8]),
            (Kind::Other, [0.4, 0.15]),
        ];
        let mut points = Vec::new();
        for (k, (kind, centre)) in centres.iter().enumerate() {
            for i in 0..150 {
                let mut p = MapPoint {
                    name: format!("{} {i}", kind.label()),
                    detail: String::new(),
                    position: [
                        centre[0] + (noise() + noise() + noise()) * 0.07,
                        centre[1] + (noise() + noise() + noise()) * 0.07,
                    ],
                    kind: *kind,
                    guessed: i % 7 == 0,
                    by_hand: false,
                    in_use: i == 3 && k % 2 == 0,
                    selected: false,
                    matches: true,
                    preset: i % 25 == 0,
                    pinned: false,
                    tags: Vec::new(),
                };
                p.position = [p.position[0].clamp(0.0, 1.0), p.position[1].clamp(0.0, 1.0)];
                points.push(p);
            }
        }
        let positions: Vec<[f32; 2]> = points.iter().map(|p| p.position).collect();
        let grid = suhina_soundmap::grid::snap(&positions, 1.0);
        for layout in [Layout::Cloud, Layout::Grid] {
            let view = MapView { layout, ..MapView::default() };
            let mut harness = Harness::builder().with_size(egui::vec2(900.0, 640.0)).build_ui_state(
                |ui, view: &mut MapView| {
                    crate::kit::theme::install(ui.ctx());
                    let mut events = Vec::new();
                    show(ui, view, &points, Some(&grid), "1 200 samples", &[], None, &[], &mut events);
                },
                view,
            );
            harness.run();
            let image = harness.render().unwrap();
            let name = format!("../../target/sample-map-{layout:?}.png").to_lowercase();
            image.save(name).unwrap();
        }
    }

    /// Renders 200 000 points (the density view) to `target/sample-map-crowded.png`.
    #[test]
    #[ignore]
    fn render_a_crowded_map() {
        use egui_kittest::Harness;
        let points = super::many::points(200_000);
        let mut harness = Harness::builder().with_size(egui::vec2(900.0, 640.0)).build_ui_state(
            |ui, view: &mut MapView| {
                crate::kit::theme::install(ui.ctx());
                let mut events = Vec::new();
                show(ui, view, &points, None, "200 000 samples", &[], None, &[], &mut events);
            },
            MapView::default(),
        );
        harness.run();
        harness.render().unwrap().save("../../target/sample-map-crowded.png").unwrap();
    }
}
