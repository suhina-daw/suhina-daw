// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Draws the browser panel (`docs/spec/browser.md` › Layout; sizes from
//! `docs/design/components.md` › Browser): the search, the category filters, the sidebar of
//! places, the results, the preview strip and the footer. Library changes and previews run
//! their commands (`library.*`, `preview.*`); adding to the project is left to the window,
//! which knows the selected track.

use std::path::PathBuf;

use egui::{Align2, Color32, CursorIcon, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_library::{FolderState, Group, Item, SectionKind};
use suhina_session::{Category, Projects};

use crate::browser::{BrowserState, Place, Row, duration_text};
use crate::interaction::drop::{BrowserItem, ItemKind};
use crate::interaction::status::Status;
use crate::kit::element::{
    BROWSER, BROWSER_PLACE, BROWSER_PLAY, BROWSER_ROW as BROWSER_ROW_ELEMENT, BUTTON, Checked, DIVIDER, Entered,
    Native, Selected,
};
use crate::kit::theme::{self, BROWSER_ROW as ROW, PAD};
use crate::kit::widgets::play_glyph;
use crate::kit::{motion, widgets};
use crate::settings::Settings;

pub const SEARCH: f32 = 26.0;
pub const FILTER: f32 = 16.0;
pub const SIDEBAR: f32 = 104.0;
pub const INDENT: f32 = 12.0;
/// How far the Tags section's top-level rows sit in from the section's title: a folder's
/// chevron under the title's name ([#1254] review).
const TAGS_LEAD: f32 = 8.0;
/// The air between two of the Tags section's rows that change level: a child level and the
/// row it returns to sit a few points apart where a sibling is tight, so the nesting reads.
pub const LEVEL_GAP: f32 = 4.0;

/// Clicks left of this on a row that opens are its chevron: the triangle sits 10 pt wide,
/// 4 pt in from the row's indent.
/// The Tags section's rows, flattened in the order shown: an entry, how deep it sits and
/// the folder it is under ([#1254] › Tag folders). A closed folder's own rows are not
/// listed: they show when the folder opens.
#[derive(Debug, Clone)]
struct TagRow {
    entry: suhina_library::TagEntry,
    depth: usize,
}

fn tag_rows_of(entries: &[suhina_library::TagEntry], depth: usize, out: &mut Vec<TagRow>) {
    // A level shows its folders on top and tags under them, each in the forest's own
    // order ([#1254] review: folders lead, tags follow).
    for entry in entries {
        if let suhina_library::TagEntry::Folder { folder, tags } = entry {
            if folder.open {
                // The row keeps its whole entry: what its title says it counts and what
                // Ctrl-click adds come from it ([#1254] review); its children show by
                // their own rows.
                out.push(TagRow { entry: entry.clone(), depth });
                tag_rows_of(tags, depth + 1, out);
            } else {
                out.push(TagRow { entry: entry.clone(), depth });
            }
        }
    }
    for entry in entries {
        if let suhina_library::TagEntry::Tag(_) = entry {
            out.push(TagRow { entry: entry.clone(), depth });
        }
    }
}

/// The tags a folder holds, its subfolders' under them, in the order shown ([#1254]).
fn collect_folder_tag_ids(entries: &[suhina_library::TagEntry], out: &mut Vec<u32>) {
    for entry in entries {
        match entry {
            suhina_library::TagEntry::Tag(tag) => out.push(tag.id),
            suhina_library::TagEntry::Folder { tags, .. } => collect_folder_tag_ids(tags, out),
        }
    }
}

/// The colour a row's dot draws with: a tag's own; a folder draws its mark (no colour).
fn tag_row_color(entry: &suhina_library::TagEntry) -> &str {
    match entry {
        suhina_library::TagEntry::Tag(tag) => &tag.color,
        suhina_library::TagEntry::Folder { .. } => "",
    }
}

/// Whether the flattened row with `id` is a folder's ([#1254]: a carried entry dropped on a
/// folder goes inside it).
fn folder_row_is_folder(rows: &[TagRow], id: u32) -> bool {
    rows.iter().find(|r| r.entry.id() == id).is_some_and(|r| matches!(r.entry, suhina_library::TagEntry::Folder { .. }))
}

fn chevron_zone(left: f32, depth: usize) -> f32 {
    left + 16.0 + depth as f32 * INDENT
}
pub const PREVIEW: f32 = 54.0;
pub const FOOTER: f32 = 20.0;
const FILTER_PAD: f32 = 7.0;
const FILTER_GAP: f32 = 2.0;
const DOT: f32 = 6.0;
/// A row's tag dot answers a hover with a hit area this wide, 4 pt larger than the dot itself.
/// Adjacent dots sit one hit area apart, so their areas touch but never overlap: a pointer
/// nearer one dot cannot answer with the next dot's tag.
const DOT_HIT: f32 = DOT + 4.0;
const PLAY: f32 = 22.0;
/// The strip above the results that holds the Plug-ins category's Instruments | Effects switch,
/// and the strip of its columns' headings.
const MODE_SWITCH: f32 = 22.0;
/// The narrowest the name column of the plug-ins' list gets: below it, the columns to its right
/// drop, the rightmost first, so the name always shows.
const NAME_LEAST: f32 = 96.0;
/// The name of the button that shows the sample map, after the categories.
const SAMPLE_MAP: &str = "Sample map";
/// The center of a row's tag dot `index`, `5` pt right of the item's name. Two dots are
/// [`DOT_HIT`] apart, the width of their hit area, so the areas never overlap.
fn tag_dot_center(name_end: f32, index: usize, y: f32) -> Pos2 {
    Pos2::new(name_end + 5.0 + index as f32 * DOT_HIT, y)
}

/// What the window has to do for the browser.
#[derive(Debug, Default)]
pub struct BrowserOut {
    /// Items to add to the selected track (Enter, double-click).
    pub add: Vec<BrowserItem>,
    /// A project file to open.
    pub open_project: Option<PathBuf>,
    pub status: Option<Status>,
    /// Browse… beside the new folder's field: the system's folder dialog.
    pub pick_folder: bool,
    /// The preview strip's Auto switch was clicked: turn the automatic preview on or off.
    pub toggle_autoplay: bool,
    /// Copy to folder… in a row's menu: these files, into a folder the system's dialog picks
    /// (`library.copyTo`, #865).
    pub copy_to: Vec<PathBuf>,
}

/// Draws the results area another way: the area, the browser's state and the projects.
pub type OtherResults<'a> = dyn FnMut(&mut Ui, Rect, &mut BrowserState, &mut Projects) + 'a;

/// What the browser needs from the window besides its own state.
pub struct Context<'a> {
    pub projects: &'a mut Projects,
    pub settings: &'a Settings,
    /// The track a MIDI file previews through.
    pub selected_track: Option<TrackId>,
    /// The key shown in the search field (Ctrl+F).
    pub search_key: String,
    /// Draws the results area another way while [`BrowserState::mode`] is not the list (a
    /// view of the files laid out by how they sound, for one).
    pub other_results: Option<&'a mut OtherResults<'a>>,
}

fn icon_color(kind: ItemKind) -> Color32 {
    match kind {
        ItemKind::Midi | ItemKind::MidiEffect => theme::track_color_2(),
        ItemKind::Sample => theme::track_color_3(),
        ItemKind::Instrument | ItemKind::Preset => theme::track_color_4(),
        ItemKind::Effect | ItemKind::AudioEffect => theme::track_color_1(),
        ItemKind::Folder | ItemKind::Heading | ItemKind::Project => theme::browser_text_secondary(),
    }
}

/// Our own simple stroke icons, 12 px.
fn icon(painter: &egui::Painter, rect: Rect, kind: ItemKind, color: Color32) {
    let stroke = Stroke::new(1.1, color);
    let c = rect.center();
    match kind {
        ItemKind::Folder | ItemKind::Heading => {
            let body = Rect::from_center_size(c + Vec2::new(0.0, 1.0), Vec2::new(10.0, 7.0));
            painter.rect_stroke(body, 0.0, stroke, egui::StrokeKind::Inside);
            painter.line_segment([body.left_top(), body.left_top() + Vec2::new(4.0, 0.0)], stroke);
            painter
                .line_segment([body.left_top() + Vec2::new(0.0, -1.5), body.left_top() + Vec2::new(3.5, -1.5)], stroke);
        }
        ItemKind::Sample => {
            for (dx, h) in [(-4.0, 2.0), (-2.0, 5.0), (0.0, 8.0), (2.0, 4.0), (4.0, 2.5)] {
                painter.line_segment([c + Vec2::new(dx, -h / 2.0), c + Vec2::new(dx, h / 2.0)], stroke);
            }
        }
        ItemKind::Midi => {
            painter.circle_stroke(c + Vec2::new(-1.5, 2.5), 2.0, stroke);
            painter.line_segment([c + Vec2::new(0.5, 2.5), c + Vec2::new(0.5, -4.5)], stroke);
            painter.line_segment([c + Vec2::new(0.5, -4.5), c + Vec2::new(3.5, -3.0)], stroke);
        }
        ItemKind::Instrument | ItemKind::Effect | ItemKind::AudioEffect | ItemKind::MidiEffect => {
            let body = Rect::from_center_size(c, Vec2::splat(9.0));
            painter.rect_stroke(body, 0.0, stroke, egui::StrokeKind::Inside);
            painter.circle_stroke(c, 2.0, stroke);
        }
        ItemKind::Preset => {
            for (dx, y) in [(-3.0, -1.0), (0.0, 2.0), (3.0, -2.0)] {
                painter.line_segment([c + Vec2::new(dx, -4.5), c + Vec2::new(dx, 4.5)], stroke);
                painter.line_segment([c + Vec2::new(dx - 1.5, y), c + Vec2::new(dx + 1.5, y)], stroke);
            }
        }
        ItemKind::Project => {
            let page = Rect::from_center_size(c, Vec2::new(8.0, 10.0));
            painter.rect_stroke(page, 0.0, stroke, egui::StrokeKind::Inside);
            painter
                .line_segment([page.left_top() + Vec2::new(2.0, 3.5), page.right_top() + Vec2::new(-2.0, 3.5)], stroke);
            painter
                .line_segment([page.left_top() + Vec2::new(2.0, 6.0), page.right_top() + Vec2::new(-2.0, 6.0)], stroke);
        }
    }
}

fn tag_color(text: &str) -> Color32 {
    theme::parse_hex(text).unwrap_or_else(theme::browser_text_hint)
}

/// Runs a library or preview command for the browser; an error goes to the status bar.
fn run(projects: &mut Projects, id: &str, params: serde_json::Value, out: &mut BrowserOut) -> bool {
    run_reply(projects, id, params, out).is_some()
}

/// [`run`], with the command's reply when it ran.
fn run_reply(
    projects: &mut Projects,
    id: &str,
    params: serde_json::Value,
    out: &mut BrowserOut,
) -> Option<serde_json::Value> {
    match projects.execute(id, &params) {
        Ok(reply) => Some(reply),
        Err(e) => {
            suhina_plugins::trace!("{} refused {} — {}", id, params, e.message());
            out.status = Some(Status::Error(e.message().to_owned()));
            None
        }
    }
}

/// Makes a tag folder with `library.addTagFolder`: the made folder's row is shown once the
/// sidebar lays it out ([#1254] review). Whether it was made.
fn add_tag_folder(
    state: &mut BrowserState,
    projects: &mut Projects,
    params: serde_json::Value,
    out: &mut BrowserOut,
) -> bool {
    let Some(reply) = run_reply(projects, "library.addTagFolder", params, out) else { return false };
    state.reveal_tag_row =
        reply.get("folder").and_then(serde_json::Value::as_u64).and_then(|id| u32::try_from(id).ok());
    true
}

/// The tag `name` was just added: its row is shown once the sidebar lays it out, scrolled
/// into view however many rows come before it ([#1254] review).
fn reveal_added_tag(state: &mut BrowserState, projects: &mut Projects, name: &str) {
    state.reveal_tag_row = projects.file_library_mut().tag_id_of(name);
}

/// Draws the browser in `area`.
pub fn show(ui: &mut Ui, area: Rect, state: &mut BrowserState, cx: Context<'_>) -> BrowserOut {
    let style = BROWSER.enter();
    let mut styled_ui = style.ui(ui);
    let ui = &mut *styled_ui;

    let mut out = BrowserOut::default();
    let Context { projects, settings, selected_track, search_key, other_results } = cx;
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    // The panel's own background, over the window's (#1233).
    let _ = style.paint_with(
        &painter,
        area,
        Native { radius: 0.0, fill: crate::kit::style_class::surface_or(theme::panel_background()) },
    );
    state.poll();
    suhina_plugins::trace::timed("library poll", || projects.file_library_mut().poll());

    // The search.
    let search = Rect::from_min_size(area.min, Vec2::new(area.width(), SEARCH));
    search_field(ui, search, state, &search_key, &mut out);
    if std::mem::take(&mut state.stop_preview) {
        stop_preview(state, projects);
    }

    // The sidebar alone chooses what the list shows (#711). Over the list: Recent's kinds as
    // chips while Recent is chosen and the setting says chips, and the List / Map switch while
    // the place can hold samples: Library › Samples, a folder, Pinned, Recent (#862).
    let mut y = search.bottom() + 4.0;
    let mut x = area.left() + PAD;
    let small = theme::ui_font(theme::TEXT_SMALL);
    let map = other_results.is_some();
    let recent = matches!(state.place, Some(Place::Recent(_)));
    let samples = state.place.as_ref().map_or(state.category == Category::Samples, Place::holds_samples);
    let mut chips: Vec<(String, Chip)> = if recent && settings.view.recent_chips {
        std::iter::once(("All".to_string(), Chip::Recent(None)))
            .chain(Group::ALL.into_iter().map(|g| (g.title().to_string(), Chip::Recent(Some(g)))))
            .collect()
    } else {
        Vec::new()
    };
    if samples && map {
        chips.extend([("List".to_string(), Chip::List), (SAMPLE_MAP.to_string(), Chip::Map)]);
    }
    let showing_map = state.mode == crate::browser::ResultsMode::Other;
    for (title, chip) in &chips {
        // The List / Sample map switch starts its own row after Recent's kinds, so it doesn't
        // read as two more kinds.
        if matches!(chip, Chip::List) && x > area.left() + PAD {
            x = area.left() + PAD;
            y += FILTER + FILTER_GAP;
        }
        let width = widgets::text_width(ui, title, small.clone()) + FILTER_PAD * 2.0;
        if x + width > area.right() - PAD && x > area.left() + PAD {
            x = area.left() + PAD;
            y += FILTER + FILTER_GAP;
        }
        let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, FILTER));
        let label = match chip {
            Chip::Recent(_) => format!("Recent {title}"),
            Chip::List => "Show the samples as a list".to_string(),
            Chip::Map => "Show the samples laid out by how they sound".to_string(),
        };
        let response = widgets::button_area(ui, rect, Id::unique(("browser-filter", title.as_str())), label);
        let chosen = match chip {
            Chip::Recent(group) => state.place == Some(Place::Recent(*group)),
            Chip::List => !showing_map,
            Chip::Map => showing_map,
        };
        let el = BUTTON.response(&response).attr(Checked, chosen).enter();
        if response.clicked() {
            match chip {
                Chip::Recent(group) => {
                    state.place = Some(Place::Recent(*group));
                    state.category = Place::Recent(*group).category().unwrap_or(Category::All);
                    state.mode = crate::browser::ResultsMode::List;
                }
                Chip::List => state.mode = crate::browser::ResultsMode::List,
                Chip::Map => state.mode = crate::browser::ResultsMode::Other,
            }
            state.scroll = 0.0;
        }
        let lit = motion::toward(ui.ctx(), response.id.with("on"), chosen, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        widgets::button_surface(
            &painter,
            rect,
            &el,
            &response,
            chosen,
            motion::mix(idle, theme::control_selected(), lit),
            0.0,
        );
        let text = el.color_or(motion::mix(theme::browser_text_secondary(), theme::text_inverse(), lit));
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, title, theme::ui_font(theme::TEXT_SMALL), text);
        x = rect.right() + FILTER_GAP;
    }
    // The map belongs to the samples: anywhere else the list shows.
    if !samples {
        state.mode = crate::browser::ResultsMode::List;
    }
    if chips.is_empty() {
        y -= FILTER + 2.0;
    }
    y += FILTER + 6.0;
    painter.hline(area.x_range(), y - 0.5, theme::hairline(theme::border()));

    let bottom = area.bottom() - FOOTER - PREVIEW;
    let sidebar = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(area.left() + SIDEBAR, bottom));
    let results = Rect::from_min_max(Pos2::new(sidebar.right() + 1.0, y), Pos2::new(area.right(), bottom));
    painter.vline(sidebar.right() + 0.5, sidebar.y_range(), theme::hairline(theme::border()));
    sidebar_view(ui, sidebar, state, projects, settings.view.recent_chips, &mut out);

    if let Err(e) = suhina_plugins::trace::timed("browser rows", || state.refresh_rows(projects)) {
        out.status = Some(Status::Error(e));
    }
    match (state.mode, other_results) {
        (crate::browser::ResultsMode::Other, Some(draw)) => draw(ui, results, state, projects),
        _ => results_view(ui, results, state, projects, settings, selected_track, &mut out),
    }
    let strip = Rect::from_min_max(Pos2::new(area.left(), bottom), Pos2::new(area.right(), bottom + PREVIEW));
    preview_strip(ui, strip, state, projects, settings, selected_track, &mut out);
    let footer = Rect::from_min_max(Pos2::new(area.left(), strip.bottom()), area.max);
    footer_view(ui, footer, state, projects, &mut out);

    // A selection that changed previews with auto-play.
    if std::mem::take(&mut state.selection_changed) {
        state.forget_unselected();
        let one = match state.selected.as_slice() {
            [one] if matches!(one.kind, ItemKind::Sample | ItemKind::Midi) => Some(one.clone()),
            _ => None,
        };
        match one {
            Some(item) if settings.preview_autoplay => {
                start_preview(state, projects, settings, selected_track, &item, 0.0, &mut out)
            }
            _ => stop_preview(state, projects),
        }
    }
    // A sample decoded for its preview plays now.
    if let Some((path, from, data)) = state.take_ready_preview() {
        // With no project open this opens the previews' own output (ADR 0011).
        let provided = projects.preview_session_mut().map(|session| session.provide_audio(&path, data));
        let params = json!({ "path": path.display().to_string(), "from": from, "gain": settings.preview_volume });
        match provided.and_then(|()| projects.execute("preview.play", &params)) {
            Ok(_) => state.preview_started(path.display().to_string()),
            Err(e) => out.status = Some(Status::Error(format!("Cannot preview: {}", e.message().to_owned()))),
        }
    }
    if let Some(e) = state.take_preview_error() {
        out.status = Some(Status::Error(format!("Cannot preview: {e}")));
    }
    // Whether the preview voice still plays, wherever the control for it is shown.
    let (sounding, started) = preview_sounding(projects);
    state.observe_preview(sounding, started);
    // While a sample is being read for its preview, every frame looks for it, so it plays the
    // moment it is ready.
    if state.preview_pending() {
        ui.ctx().request_repaint();
    } else if state.previewing.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    // A search on its own thread answers without an input to wake the window.
    if state.searching() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
    out
}

fn search_field(ui: &mut Ui, rect: Rect, state: &mut BrowserState, key: &str, out: &mut BrowserOut) {
    let painter = ui.painter_at(rect);
    // `.browser-search` (or `browser text-input`): its background, border, radius and `color`.
    let search_style = crate::kit::element::TEXT_INPUT.id("browser-search").enter();
    paint_row(&painter, &search_style, rect, crate::kit::style_class::surface_or(theme::window_background()));
    if let Some(style) = search_style.style().filter(|s| s.border_width > 0.0 && s.border.a() > 0) {
        painter.rect_stroke(
            rect,
            style.radius,
            Stroke::new(style.border_width, style.border),
            egui::StrokeKind::Inside,
        );
    }
    let glass = Pos2::new(rect.left() + PAD + 5.0, rect.center().y);
    painter.circle_stroke(glass + Vec2::new(-1.0, -1.0), 3.5, Stroke::new(1.1, theme::browser_text_hint()));
    painter.line_segment(
        [glass + Vec2::new(1.5, 1.5), glass + Vec2::new(4.5, 4.5)],
        Stroke::new(1.1, theme::browser_text_hint()),
    );
    let hint = theme::mono_font(theme::TEXT_TINY);
    let key_width = widgets::text_width(ui, key, hint.clone());
    let field = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + 14.0, rect.top() + 4.0),
        Pos2::new(rect.right() - PAD - key_width - 6.0, rect.bottom() - 4.0),
    );
    let id = Id::unique("browser-search");
    // egui lets go of a field's focus on Escape before the field is drawn, so whether the
    // field had the keyboard is remembered from the frame before.
    let focused = ui.ctx().memory(|m| m.has_focus(id)) || state.search_focused;
    if focused {
        // The list's keys, taken before the field sees them.
        let (down, up, enter, escape) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if down || up {
            move_selection(state, if down { 1 } else { -1 });
        }
        if enter && !state.selected.is_empty() {
            out.add = state.selected.clone();
        }
        if escape {
            state.stop_preview = true;
            if state.search.is_empty() {
                ui.ctx().memory_mut(|m| m.surrender_focus(id));
                state.search_focused = false;
            } else {
                state.search.clear();
                // Escape cleared the search; the field keeps the keyboard.
                state.focus_search = true;
            }
        }
    }
    let edit = egui::TextEdit::singleline(&mut state.search)
        .id(id)
        .frame(egui::Frame::NONE)
        .hint_text(egui::RichText::new("Search").color(theme::browser_text_hint()))
        .font(theme::ui_font(theme::TEXT))
        .text_color(search_style.color_or(theme::browser_text_primary()));
    let response = crate::kit::widgets::text_input_at(ui, field, edit);
    if std::mem::take(&mut state.focus_search) {
        response.request_focus();
    }
    if std::mem::take(&mut state.select_search)
        && let Some(mut text) = egui::TextEdit::load_state(ui.ctx(), id)
    {
        let all = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(state.search.chars().count()),
        );
        text.cursor.set_char_range(Some(all));
        text.store(ui.ctx(), id);
    }
    state.search_focused = response.has_focus();
    if response.changed() {
        state.scroll = 0.0;
    }
    widgets::text_in(
        &painter,
        Rect::from_min_max(Pos2::new(field.right(), rect.top()), rect.max - Vec2::new(PAD, 0.0)),
        Align2::RIGHT_CENTER,
        key,
        hint,
        theme::browser_text_hint(),
    );
}

/// Moves the selection `step` rows (from the search field's arrow keys).
/// The arrow keys while a row of the list was pressed last: up and down move the selection (and
/// its preview), right opens a folder or goes into an open one, left closes one or goes up to
/// the folder it is in. Taken before the window's shortcuts see them.
pub fn list_keys(ui: &Ui, state: &mut BrowserState) {
    if !state.list_focused || state.search_focused {
        return;
    }
    let (down, up, left, right) = ui.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::ArrowLeft),
            i.consume_key(Modifiers::NONE, Key::ArrowRight),
        )
    });
    if down || up {
        move_selection(state, if down { 1 } else { -1 });
    }
    if !(left || right) {
        return;
    }
    let rows = state.rows();
    let Some(current) = state.selected.last().and_then(|s| rows.iter().position(|r| r.item.key == s.key)) else {
        return;
    };
    let Some(row) = rows.get(current) else { return };
    let (opens, open, depth, node) = (row.opens, row.open, row.depth, row.node().to_string());
    if right && opens && !open {
        state.toggle(&node);
    } else if right && open {
        move_selection(state, 1);
    } else if left && open {
        state.toggle(&node);
    } else if left && let Some(parent) = rows.get(..current).and_then(|r| r.iter().rposition(|r| r.depth < depth)) {
        move_selection(state, parent as isize - current as isize);
    }
}

fn move_selection(state: &mut BrowserState, step: isize) {
    let rows = state.rows();
    if rows.is_empty() {
        return;
    }
    let current = state.selected.last().and_then(|s| rows.iter().position(|r| r.item.key == s.key));
    let next = match current {
        Some(i) => i.saturating_add_signed(step).min(rows.len() - 1),
        None => 0,
    };
    if let Some(row) = rows.get(next) {
        let item = row.item.clone();
        state.anchor = Some(next);
        state.select(vec![item]);
        state.scroll = state.scroll.min(next as f32 * ROW).max(next as f32 * ROW - ROW * 8.0);
    }
}

/// What a chip over the list stands for (#711).
#[derive(Clone, Copy)]
enum Chip {
    /// A kind of recent item, or all of them.
    Recent(Option<Group>),
    /// The samples as a list, or laid out by how they sound.
    List,
    Map,
}

/// What a sidebar item chooses: a place, or a kind of the library shown whole (#711).
enum Target {
    Place(Place),
    Category(Category),
}

/// One row of the sidebar's Pinned, Library and Folders sections: selected when its place is
/// the one shown, a `×` while the place is chosen, and a click that shows it. The row is
/// `Row` tall and consumes that of `y`.
#[allow(clippy::too_many_arguments)]
fn entry_row(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    y: &mut f32,
    state: &mut BrowserState,
    target: Target,
    label: &str,
    note: Option<&str>,
) -> egui::Response {
    let rect = Rect::from_min_size(Pos2::new(area.left(), *y), Vec2::new(area.width(), ROW));
    *y += ROW;
    let key = match &target {
        Target::Place(place) => place.param(),
        Target::Category(category) => format!("library:{}", category.title()),
    };
    let response = ui.interact(rect.intersect(area), Id::unique(("browser-place", key.as_str())), Sense::click());
    // Screen readers and tests: "Show Samples" for the library's kinds, "Place Recent MIDI"
    // for a recent kind, "Place Pinned" and the like for the rest.
    let name = match &target {
        Target::Category(category) => format!("Show {}", category.title()),
        Target::Place(Place::Recent(Some(group))) => format!("Place Recent {}", group.title()),
        Target::Place(_) => format!("Place {}", label.trim()),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    // A kind of the library is chosen while no place is: the whole of it shows.
    let chosen = match &target {
        Target::Place(place) => state.place.as_ref() == Some(place),
        Target::Category(category) => state.place.is_none() && state.category == *category,
    };
    let place = match target {
        Target::Place(place) => place,
        Target::Category(category) => {
            if response.clicked() && !chosen {
                state.place = None;
                state.category = category;
                state.scroll = 0.0;
            }
            if rect.intersects(area) {
                let style = place_style(&response, chosen);
                let fill = if chosen {
                    theme::control_active()
                } else {
                    motion::mix(egui::Color32::TRANSPARENT, theme::panel_hover(), widgets::hover(ui, &response))
                };
                paint_row(painter, &style, rect.intersect(area), fill);
                let text = Rect::from_min_max(rect.min + Vec2::new(PAD + 2.0, 0.0), rect.max - Vec2::new(PAD, 0.0));
                let color = if chosen { theme::browser_text_primary() } else { theme::browser_text_secondary() };
                let color = style.color_or(color);
                widgets::text_in(painter, text, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), color);
            }
            return response;
        }
    };
    let fill = if chosen {
        theme::control_active()
    } else {
        motion::mix(egui::Color32::TRANSPARENT, theme::panel_hover(), widgets::hover(ui, &response))
    };
    let style = place_style(&response, chosen);
    if rect.intersects(area) {
        paint_row(painter, &style, rect.intersect(area), fill);
        let text = Rect::from_min_max(rect.min + Vec2::new(PAD + 2.0, 0.0), rect.max - Vec2::new(PAD, 0.0));
        let color = if chosen { theme::browser_text_primary() } else { theme::browser_text_secondary() };
        let color = style.color_or(color);
        widgets::text_in(painter, text, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), color);
        if let Some(note) = note.filter(|_| !chosen) {
            widgets::text_in(
                painter,
                text,
                Align2::RIGHT_CENTER,
                note,
                theme::mono_font(theme::TEXT_TINY),
                theme::browser_text_hint(),
            );
        }
    }
    // The place chosen shows a × that clears it; a second click on the place keeps it.
    if chosen && rect.intersects(area) {
        let clear = Rect::from_min_size(Pos2::new(rect.right() - ROW, rect.top()), Vec2::splat(ROW));
        let cleared = widgets::button_area(
            ui,
            clear.intersect(area),
            Id::unique(("browser-place-clear", place.param())),
            "Show everything",
        );
        let color =
            motion::mix(theme::browser_text_hint(), theme::browser_text_primary(), widgets::hover(ui, &cleared));
        widgets::text_in(painter, clear, Align2::CENTER_CENTER, "×", theme::ui_font(theme::TEXT_SMALL), color);
        if cleared.clicked() {
            state.place = None;
            state.category = Category::All;
            state.scroll = 0.0;
            return response;
        }
    }
    if response.clicked() && !chosen {
        // A place lists any kind, or a recent group's own: the category follows it, so
        // nothing the place lists is hidden (#711).
        state.category = place.category().unwrap_or(Category::All);
        state.place = Some(place);
        state.scroll = 0.0;
    }
    response
}

fn sidebar_view(
    ui: &mut Ui,
    area: Rect,
    state: &mut BrowserState,
    projects: &mut Projects,
    recent_chips: bool,
    out: &mut BrowserOut,
) {
    let painter = ui.painter_at(area);
    let title_font = theme::ui_font(theme::TEXT_SMALL);
    let item_font = theme::ui_font(theme::TEXT_SMALL);
    // The rows drawn past the bottom are clipped; the wheel scrolls the whole sidebar when
    // its entries are more than its room, as the results list does.
    let mut y = area.top() - state.sidebar_scroll;
    let title = |y: &mut f32, text: &str| {
        *y += 8.0;
        let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, *y), Vec2::new(area.width() - PAD, 12.0));
        widgets::text_in(&painter, rect, Align2::LEFT_CENTER, text, title_font.clone(), theme::browser_text_hint());
        // The rows that follow paint over the title's room: three points of clearance so a
        // descender ("g" in Tags, "y" in Library) is not clipped by the first one.
        *y += 15.0;
    };
    // The sidebar's sections in the store's order, each title collapsible and carrying a
    // menu that opens, closes and reorders — Folders is not one of them: it is always last
    // ([#1254] › Sidebar sections). The drop targets for a carried tag clear at the
    // sidebar's own start: a closed section draws no rows of its own, and yesterday's
    // targets pointing through a command that closed it would land a drop on a hidden
    // tag ([#1254] review).
    state.drop_tag = None;
    state.drop_tag_row = None;
    state.drop_folder_before = None;
    state.drop_pill = None;
    let sections_list = projects.file_library_mut().sections();
    let mut slots = Vec::new();
    for (at, section) in sections_list.iter().enumerate() {
        section_title(
            ui,
            &painter,
            area,
            &mut y,
            section.kind,
            section.open,
            at,
            sections_list.clone(),
            title_font.clone(),
            &mut slots,
            state,
            projects,
            out,
        );
        if section.open {
            match section.kind {
                SectionKind::Pinned => {
                    entry_row(ui, &painter, area, &mut y, state, Target::Place(Place::Pinned), "Pinned", None);
                    entry_row(ui, &painter, area, &mut y, state, Target::Place(Place::Recent(None)), "Recent", None);
                    if !recent_chips {
                        for group in Group::ALL {
                            entry_row(
                                ui,
                                &painter,
                                area,
                                &mut y,
                                state,
                                Target::Place(Place::Recent(Some(group))),
                                &format!("    {}", group.title()),
                                None,
                            );
                        }
                    }
                }
                SectionKind::Tags => tags_section(ui, &painter, area, &mut y, state, projects, out),
                SectionKind::Library => {
                    for category in Category::ALL {
                        entry_row(
                            ui,
                            &painter,
                            area,
                            &mut y,
                            state,
                            Target::Category(category),
                            category.title(),
                            None,
                        );
                    }
                }
            }
        }
    }
    // A carried section title lands where the pointer is over another title: the pointer's
    // half decides above or below it ([#1254] review). The thin line shows the slot; Folders
    // is never a slot, and nothing outside the titles commits the move.
    if ui.input(|i| i.key_pressed(Key::Escape)) {
        state.drag_section = None;
        state.drop_section = None;
    }
    if let Some(carried) = state.drag_section {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let slot =
            slots.iter().enumerate().find(|(_, (rect, _))| pointer.is_some_and(|p| rect.intersect(area).contains(p)));
        let released_now = ui.input(|i| i.pointer.primary_released());
        match slot {
            Some((slot_at, (rect, _))) => {
                let after = pointer.is_some_and(|p| p.y > rect.center().y);
                // The line at the slot's Y: above the half's title, or under it (the last
                // one's under is the end, before the Folders section).
                let line_y = if after { rect.bottom() + 2.0 } else { rect.top() - 2.0 };
                painter.hline(area.x_range(), line_y, Stroke::new(2.0, theme::browser_text_primary()));
                // Inserting before the kind of the slot the pointer is in; the carried kind
                // does not count (its own halves target itself, which is no move).
                let before =
                    slots.iter().skip(slot_at).skip(usize::from(after)).map(|(_, k)| *k).find(|k| *k != carried);
                state.drop_section = before;
                if released_now {
                    let mut order = slots.iter().map(|(_, k)| *k).collect::<Vec<SectionKind>>();
                    order.retain(|k| *k != carried);
                    let at = before.map_or(order.len(), |b| order.iter().position(|k| *k == b).unwrap_or(order.len()));
                    order.insert(at, carried);
                    let params = json!({ "order": order.iter().map(|k| json!(k.name())).collect::<Vec<_>>() });
                    if run(projects, "library.setSections", params, out) {
                        state.relist();
                    }
                    state.drag_section = None;
                    state.drop_section = None;
                }
            }
            None => {
                // Nothing is a slot here: the drop cancels.
                state.drop_section = None;
                if released_now {
                    state.drag_section = None;
                }
            }
        }
    }
    title(&mut y, "Folders");
    let mut menus = Vec::new();
    for folder in projects.file_library_mut().folders() {
        let name = folder
            .path
            .file_name()
            .map_or_else(|| folder.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let note = match folder.state {
            FolderState::Scanning => Some("…"),
            FolderState::Polling => Some("poll"),
            FolderState::Missing => Some("gone"),
            _ => None,
        };
        let response = entry_row(
            ui,
            &painter,
            area,
            &mut y,
            state,
            Target::Place(Place::Folder(folder.path.clone())),
            &name,
            note,
        );
        menus.push((response, folder.path));
    }
    for (response, folder) in menus {
        let path = folder.display().to_string();
        response.context_menu(|ui| {
            if ui.button("Refresh").clicked() {
                run(projects, "library.refresh", json!({ "folder": path }), out);
                state.relist();
                ui.close();
            }
            if ui.button("Pin").clicked() {
                run(projects, "library.pin", json!({ "item": path }), out);
                ui.close();
            }
            if ui.button("Remove from the browser").clicked() {
                if run(projects, "library.removeFolder", json!({ "folder": path }), out)
                    && state.place == Some(Place::Folder(folder.clone()))
                {
                    state.place = None;
                }
                state.relist();
                ui.close();
            }
        });
    }
    // Adding a folder: a field for its path.
    let rect = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), ROW + 2.0));
    y += rect.height();
    match state.new_folder.as_mut() {
        None => {
            let response =
                widgets::button_area(ui, rect.intersect(area), Id::unique("browser-add-folder"), "Add folder");
            let color =
                motion::mix(theme::browser_text_hint(), theme::browser_text_primary(), widgets::hover(ui, &response));
            widgets::text_in(
                &painter,
                rect.shrink2(Vec2::new(PAD + 2.0, 0.0)),
                Align2::LEFT_CENTER,
                "+ Add folder",
                item_font.clone(),
                color,
            );
            if response.clicked() {
                state.new_folder = Some(String::new());
            }
        }
        Some(text) => {
            let id = Id::unique("browser-new-folder");
            let edit = egui::TextEdit::singleline(text)
                .id(id)
                .hint_text("A folder's path")
                .font(item_font.clone())
                .text_color(theme::browser_text_primary());
            let field = rect.shrink2(Vec2::new(4.0, 1.0));
            let browse =
                Rect::from_min_size(Pos2::new(field.right() - 56.0, field.top()), Vec2::new(56.0, field.height()));
            if crate::kit::widgets::button(ui, &painter, browse.intersect(area), "Browse…", "Browse for a folder") {
                out.pick_folder = true;
                state.new_folder = None;
                return;
            }
            let response = crate::kit::widgets::text_input_at(
                ui,
                Rect::from_min_max(field.min, Pos2::new(browse.left() - 4.0, field.bottom())).intersect(area),
                edit,
            );
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if response.lost_focus() || escape {
                let path = text.trim().to_string();
                if enter && !path.is_empty() {
                    if run(projects, "library.addFolder", json!({ "folder": path }), out) {
                        out.status = Some(Status::Done(format!("Added {path} to the browser")));
                    }
                    state.relist();
                }
                state.new_folder = None;
            }
        }
    }
    // The wheel scrolls the sidebar while some of it lies past the bottom (tags, folders), so
    // the first enters and the last stay reachable: rows above carry the scroll into y, so
    // the content height takes it back out ([#1196]).
    let content = (y - area.top() + state.sidebar_scroll).max(0.0);
    let max_scroll = (content - area.height()).max(0.0);
    if ui.rect_contains_pointer(area) {
        state.sidebar_scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    // A row just added scrolls into view ([#1254] review); one waiting for a Tags section
    // that stays closed is let go.
    if let Some(by) = state.sidebar_reveal_by.take() {
        state.sidebar_scroll += by;
    }
    if !projects.file_library_mut().sections().iter().any(|s| s.kind == SectionKind::Tags && s.open) {
        state.reveal_tag_row = None;
    }
    state.sidebar_scroll = state.sidebar_scroll.clamp(0.0, max_scroll);
}

/// How far the sidebar's scroll moves so `row` shows inside `area`: down by what lies under
/// the bottom edge, up by what lies over the top; `None` when it shows already.
fn reveal_shift(row: Rect, area: Rect) -> Option<f32> {
    if row.bottom() > area.bottom() {
        Some(row.bottom() - area.bottom())
    } else if row.top() < area.top() {
        Some(row.top() - area.top())
    } else {
        None
    }
}

fn results_view(
    ui: &mut Ui,
    mut area: Rect,
    state: &mut BrowserState,
    projects: &mut Projects,
    settings: &Settings,
    selected_track: Option<TrackId>,
    out: &mut BrowserOut,
) {
    // On the browser's own background, which `show` painted.
    // The Plug-ins category's Instruments | Effects switch takes the results' top, then its
    // list's column headings, unless a place or a search lists other rows.
    let plugins_view = projects.file_library_mut().plugins_view();
    let listed = state.mode == crate::browser::ResultsMode::List
        && state.category == Category::Plugins
        && state.place.is_none()
        && state.search.trim().is_empty()
        && state.tag_filter.is_empty();
    if listed {
        let strip = Rect::from_min_size(area.min, Vec2::new(area.width(), MODE_SWITCH));
        plugins_view_switch(ui, strip, projects, plugins_view, out);
        area = area.intersect(Rect::from_min_max(Pos2::new(area.left(), strip.bottom() + 2.0), area.max));
    }
    if listed {
        // The columns' header row, its headings clickable to sort.
        let strip = Rect::from_min_size(area.min, Vec2::new(area.width(), MODE_SWITCH));
        columns_header(ui, strip, state);
        area = area.intersect(Rect::from_min_max(Pos2::new(area.left(), strip.bottom() + 2.0), area.max));
    }
    let painter = ui.painter_at(area);
    let rows: Vec<Row> = state.rows().to_vec();
    // The drop targets are this frame's own, cleared before an empty list returns: a target
    // an outside change emptied the list under would otherwise still take the drop
    // ([#1254] review).
    state.drop_folder = None;
    state.drop_item = None;
    if rows.is_empty() {
        let folders = projects.file_library_mut().folders().len();
        let tags = matches!(&state.place, Some(Place::Tag(_)));
        // A place's own empty answer needs the search text or a filter out of the way first:
        // the tags are assigned, the list is what excludes them ([#1196] › the filtered
        // empty answer).
        let filtered = !state.search.trim().is_empty() || !state.tag_filter.is_empty();
        let text = if state.search.trim().is_empty() && folders == 0 && !tags {
            "Add a folder to find samples"
        } else if tags && filtered {
            "Nothing found here with these filters"
        } else if tags {
            "No item carries these tags"
        } else {
            "Nothing found"
        };
        widgets::text_in(
            &painter,
            area.shrink(PAD),
            Align2::CENTER_TOP,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            theme::browser_text_hint(),
        );
        return;
    }
    // Scrolling with the wheel over the list.
    let content = rows.len() as f32 * ROW;
    let max_scroll = (content - area.height()).max(0.0);
    if ui.rect_contains_pointer(area) {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        state.scroll -= wheel;
    }
    state.scroll = state.scroll.clamp(0.0, max_scroll);
    let first = (state.scroll / ROW).floor() as usize;
    let tags = projects.file_library_mut().tag_flat();
    let visible = (area.height() / ROW).ceil() as usize + 1;
    let mono = theme::mono_font(theme::TEXT_TINY);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut clicked: Option<(usize, Modifiers)> = None;
    let mut pressed: Option<(usize, Modifiers)> = None;
    let mut double: Option<usize> = None;
    let mut drag: Option<usize> = None;
    let mut play: Option<usize> = None;
    // Samples dragged over a folder row go into that folder when dropped (#943).
    let dragging_samples = state.drag.as_ref().is_some_and(|items| items.iter().any(|i| i.kind == ItemKind::Sample));
    let pointer = ui.input(|i| i.pointer.latest_pos());
    for index in first..(first + visible).min(rows.len()) {
        let Some(row) = rows.get(index) else { continue };
        let top = area.top() + index as f32 * ROW - state.scroll;
        let rect = Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), ROW));
        let shown = rect.intersect(area);
        if shown.height() <= 0.0 {
            continue;
        }
        let response = ui.interact(shown, Id::unique(("browser-row", row.node())), Sense::click_and_drag());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Browser {}", row.item.name)));
        let selected = state.selected.iter().any(|s| s.key == row.item.key);
        // `browser-row`, with `:hover`, `:active` and `[selected]`; its `color` is the name's.
        let row_style = BROWSER_ROW_ELEMENT.response(&response).attr(Selected, selected).enter();
        let drop_here = dragging_samples
            && row.item.kind == ItemKind::Folder
            && pointer.is_some_and(|p| shown.contains(p))
            && row.item.path().is_some_and(|p| p.is_dir());
        if drop_here {
            state.drop_folder = row.item.path();
        }
        // A tag carried from the Tags section highlights the item it is dropped on to tag it:
        // anything but a heading, a folder as a whole among them, as a row's Tag menu does
        // (#1196, [#1254] review). A drag owns the pointer, so the row is found by position,
        // the folder drops below in the same way.
        let tag_here = state.drag_tag.is_some()
            && row.item.kind != ItemKind::Heading
            && pointer.is_some_and(|p| shown.contains(p));
        if tag_here {
            state.drop_item = Some(row.item.clone());
        }
        let fill = if drop_here || tag_here {
            theme::control_hover()
        } else if selected {
            theme::control_active()
        } else {
            motion::mix(egui::Color32::TRANSPARENT, theme::panel_hover(), widgets::hover(ui, &response))
        };
        paint_row(&painter, &row_style, shown, fill);
        let mut x = rect.left() + 4.0 + row.depth as f32 * INDENT;
        let lead = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(10.0, ROW));
        let previewable = matches!(row.item.kind, ItemKind::Sample | ItemKind::Midi);
        if row.opens {
            widgets::chevron(&painter, lead.center(), row.open, theme::browser_text_hint());
        } else if previewable && (selected || response.hovered()) {
            let button = widgets::button_area(
                ui,
                lead,
                Id::unique(("browser-play", row.node())),
                format!("Preview {}", row.item.name),
            );
            let playing = state.preview_active_for(&row.item.key);
            // `browser-play`, `[checked]` while it plays, `color` its glyph.
            let play_style = BROWSER_PLAY.id("browser-play").response(&button).attr(Checked, playing).enter();
            let ink = if button.hovered() { theme::browser_text_primary() } else { theme::browser_text_secondary() };
            play_glyph(&painter, lead.center(), playing, play_style.color_or(ink), 7.0);
            if button.clicked() {
                play = Some(index);
            }
        }
        x = lead.right() + 2.0;
        let icon_rect = Rect::from_min_size(Pos2::new(x, rect.top() + 2.5), Vec2::splat(12.0));
        icon(&painter, icon_rect, row.item.kind, icon_color(row.item.kind));
        x = icon_rect.right() + 5.0;
        // The plug-ins' list's columns, those there is room for.
        if let Some(columns) = &row.columns {
            let small = theme::ui_font(theme::TEXT_TINY);
            if let Some((left, right)) = column_span(area, 1) {
                let span = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom()));
                widgets::text_in(
                    &painter,
                    span,
                    Align2::RIGHT_CENTER,
                    columns.format,
                    small,
                    theme::browser_text_hint(),
                );
            }
        }
        // On the right: a sample's length, a device's kind, where a search found it. The
        // plug-ins' list says what each row is in its columns instead.
        let meta = match row.item.kind {
            _ if row.columns.is_some() => None,
            ItemKind::Sample | ItemKind::Midi => {
                row.item.path().and_then(|p| state.info(&p, false).and_then(|i| i.seconds)).map(duration_text)
            }
            ItemKind::Instrument if row.item.plugin => Some("plug-in".into()),
            ItemKind::Effect if row.item.plugin => Some("plug-in".into()),
            ItemKind::AudioEffect => Some("AudioEffect".into()),
            ItemKind::MidiEffect => Some("MIDI effect".into()),
            _ => None,
        }
        .unwrap_or_default();
        let name_right = match row.columns {
            Some(_) => column_span(area, 0).map_or(rect.right() - PAD, |(_, right)| right),
            None => rect.right() - PAD,
        };
        let meta_width = if meta.is_empty() {
            0.0
        } else {
            widgets::text_width(ui, &meta, mono.clone()).min(area.width() * 0.4) + PAD
        };
        let name_space = (name_right - meta_width - x).max(0.0);
        let item_tags = projects.file_library_mut().item_tags(&row.item.key);
        let dots = item_tags.len() as f32 * DOT_HIT;
        let color =
            row_style.color_or(if selected { theme::browser_text_primary() } else { theme::browser_text_value() });
        let galley = widgets::fitted(&painter, &row.item.name, font.clone(), color, (name_space - dots).max(0.0));
        let name_pos = Pos2::new(x, rect.center().y - galley.size().y / 2.0);
        let name_end = name_pos.x + galley.size().x;
        widgets::paint_text(&painter, name_pos, galley, color);
        for (i, tag_id) in item_tags.iter().enumerate() {
            if let Some(tag) = tags.iter().find(|t| t.id == *tag_id) {
                let center = tag_dot_center(name_end, i, rect.center().y);
                // `browser-tag-dot { opacity; border-color }`; its fill stays the tag's colour.
                let dot = crate::kit::element::BROWSER_TAG_DOT.enter();
                let style = dot.style();
                let fill = tag_color(&tag.color).gamma_multiply(style.as_ref().map_or(1.0, |s| s.opacity));
                painter.circle_filled(center, DOT / 2.0, fill);
                if let Some(style) = style.filter(|s| s.border_width > 0.0) {
                    painter.circle_stroke(center, DOT / 2.0, Stroke::new(style.border_width, style.border));
                }
                // The dot answers a hover with its tag's name: a tooltip, the status bar, and
                // an accessible label.
                let dot = ui
                    .interact(
                        Rect::from_center_size(center, Vec2::splat(DOT_HIT)),
                        Id::unique(("browser-tag-dot", row.node(), tag.id)),
                        Sense::hover(),
                    )
                    .on_hover_text(format!("Tag {}", tag.name));
                dot.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Tagged {}", tag.name)));
                if dot.hovered() {
                    out.status = Some(Status::Hint(format!("Tag {}", tag.name)));
                }
            }
        }
        if !meta.is_empty() {
            let meta_rect =
                Rect::from_min_max(Pos2::new(rect.right() - meta_width, rect.top()), rect.max - Vec2::new(PAD, 0.0));
            widgets::text_in(
                &painter,
                meta_rect,
                Align2::RIGHT_CENTER,
                &meta,
                mono.clone(),
                theme::browser_text_hint(),
            );
        }
        if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            pressed = Some((index, ui.input(|i| i.modifiers)));
        }
        if response.clicked() {
            clicked = Some((index, ui.input(|i| i.modifiers)));
        }
        if response.double_clicked() {
            double = Some(index);
        }
        if response.drag_started() {
            drag = Some(index);
            state.handled_press = None;
        }
        let item = row.item.clone();
        // Clicks inside the menu do not close it: its Tag submenu holds the field a new tag
        // is typed into (egui's default closes a menu on a click inside it too). A heading
        // (Instruments, Effects, Plug-ins) is no item and has no menu: a right-click on it
        // opened an empty one.
        if item.kind != ItemKind::Heading {
            egui::Popup::context_menu(&response)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| row_menu(ui, &item, &tags, state, projects, out));
        }
    }
    // A row is selected (and previews) when the button goes down, not when it comes up, so a
    // preview starts at once. A row already in a selection of several waits for the release, so
    // pressing it to drag them all keeps the selection.
    if let Some((index, modifiers)) = pressed
        && let Some(row) = rows.get(index)
    {
        let in_selection = state.selected.iter().any(|s| s.key == row.item.key);
        if !in_selection || state.selected.len() == 1 || modifiers.command || modifiers.shift {
            click_row(state, &rows, index, modifiers);
            state.handled_press = Some(row.item.key.clone());
        }
        state.list_focused = true;
    }
    if let Some((index, modifiers)) = clicked {
        let key = rows.get(index).map(|r| r.item.key.clone());
        // The press of this click selected the row already.
        if state.handled_press.take() != key {
            click_row(state, &rows, index, modifiers);
        }
        // A heading or a folder opens with a click on its name: the same rules as its chevron
        // (right below), which takes the clicks in the chevron zone instead of this one (#869).
        if let Some(row) = rows.get(index)
            && opens_on_click(row)
            && !ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
            && !ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.x < chevron_zone(area.left(), row.depth))
        {
            state.toggle(row.node());
        }
    }
    if let Some(index) = double
        && let Some(row) = rows.get(index)
    {
        // A double-click on the row itself opens a preset bank — but not on its chevron: the
        // gesture's two clicks are within egui's double-click window (0.3 s, 6 pt) and its
        // first one toggled already, so the double would open and close it again (#293); and
        // not on a heading or a folder, whose single clicks toggle above, so a double-click
        // leaves it open. A device that opens on its presets is added, as any device is.
        let on_chevron = row.opens
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.x < chevron_zone(area.left(), row.depth));
        if on_chevron && row.item.kind != ItemKind::Preset {
            // Its first click toggled it, and adding it as well would be a second action.
        } else if row.opens && !on_chevron && row.item.kind == ItemKind::Preset {
            state.toggle(row.node());
        } else if row.item.kind == ItemKind::Project {
            out.open_project = row.item.path();
        } else if row.item.kind == ItemKind::Folder && !row.opens {
            // A folder a flat list holds (a pinned or tagged one) opens as the place: the
            // tree shows its contents ([#1254] review).
            if let Some(path) = row.item.path() {
                state.place = Some(Place::Folder(path));
                state.category = Category::All;
                state.scroll = 0.0;
            }
        } else if row.item.kind != ItemKind::Heading && row.item.kind != ItemKind::Folder {
            out.add = vec![row.item.clone()];
        }
    }
    // A click on a chevron opens; a click elsewhere on a heading or a folder opens it too
    // (above), on a preset bank only selects it. The second click of a rapid pair on the same
    // chevron is a double and does not toggle: the first click of the gesture did, so the two
    // together open the folder once (#293).
    if let Some((index, _)) = clicked
        && let Some(row) = rows.get(index)
        && row.opens
        && !ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
        && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.x < chevron_zone(area.left(), row.depth))
    {
        state.toggle(row.node());
    }
    if let Some(index) = play
        && let Some(row) = rows.get(index)
    {
        if state.preview_active_for(&row.item.key) {
            stop_preview(state, projects);
        } else {
            start_preview(state, projects, settings, selected_track, &row.item.clone(), 0.0, out);
        }
    }
    if let Some(index) = drag
        && let Some(row) = rows.get(index)
    {
        let items = if state.selected.iter().any(|s| s.key == row.item.key) {
            state.selected.clone()
        } else {
            vec![row.item.clone()]
        };
        // A folder is carried too: a tag row it is dropped on tags the folder as a whole; the
        // project and the file manager take only the files ([#1254] review).
        let items: Vec<BrowserItem> = items.into_iter().filter(|i| i.kind != ItemKind::Heading).collect();
        if !items.is_empty() {
            // The preview goes on while dragging (a press held still also starts a drag); it
            // stops when the items are dropped into the project.
            state.start_drag(items);
        }
    }
    // The scroll bar every scroll area has, dragged and clicked like a device panel's; after
    // the rows, so it takes the pointer over them (#867).
    crate::kit::scroll::vertical_bar(ui, area, "browser-results-scroll", content, &mut state.scroll);
}

/// A heading or a folder opens and closes with a click anywhere on it; a preset bank only with
/// its chevron or a double-click, as a click on it previews it (#869).
fn opens_on_click(row: &Row) -> bool {
    row.opens && matches!(row.item.kind, ItemKind::Heading | ItemKind::Folder)
}

fn click_row(state: &mut BrowserState, rows: &[Row], index: usize, modifiers: Modifiers) {
    let Some(row) = rows.get(index) else { return };
    // A heading toggles in the click handling below, through the same paths a folder's
    // chevron takes, so that a click toggles it once however the pointer counts it; it never
    // selects.
    if row.item.kind == ItemKind::Heading {
        return;
    }
    if modifiers.command {
        let mut selected = state.selected.clone();
        match selected.iter().position(|s| s.key == row.item.key) {
            Some(i) => {
                selected.remove(i);
            }
            None => selected.push(row.item.clone()),
        }
        state.anchor = Some(index);
        state.select(selected);
    } else if modifiers.shift
        && let Some(anchor) = state.anchor
    {
        let (a, b) = (anchor.min(index), anchor.max(index));
        let range: Vec<BrowserItem> = rows
            .get(a..=b)
            .unwrap_or_default()
            .iter()
            .filter(|r| r.item.kind != ItemKind::Heading)
            .map(|r| r.item.clone())
            .collect();
        state.select(range);
    } else {
        state.anchor = Some(index);
        if state.selected.len() != 1 || state.selected.first().map(|s| &s.key) != Some(&row.item.key) {
            state.select(vec![row.item.clone()]);
        }
    }
}

/// A folder's right-click menu ([#1254] › Tag folders): its name typed to rename it, tags
/// and folders to add under it, and Remove — asking first, saying what moves up.
fn tag_folder_menu(
    ui: &mut Ui,
    folder_id: u32,
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
) {
    let my = format!("#{folder_id}");
    // The tag menus' state keys on the library's epoch too: a replaced library's tag of the
    // same id opens none of the old one's menus or drafts ([#1254] review).
    let epoch = state.tag_epoch;
    let rename_id = Id::unique(("browser-tag-folder", epoch, folder_id));
    // Its name, typed in, applied as it is typed: the folder command's to own ([#1254]).
    let store_name = {
        let library = projects.file_library_mut();
        suhina_library::store::folder_by_id(&library.tags(), folder_id).map(|f| f.name.clone())
    };
    let Some(store_name) = store_name else { return };
    let mut name: String = ui.ctx().data(|d| d.get_temp(rename_id)).unwrap_or_else(|| store_name.clone());
    let field =
        crate::kit::widgets::text_input(ui, egui::TextEdit::singleline(&mut name).id(rename_id).desired_width(120.0));
    field.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("Rename folder {}", store_name))
    });
    ui.ctx().data_mut(|d| d.insert_temp(rename_id, name.clone()));
    if !name.trim().is_empty()
        && name.trim() != store_name
        && run(projects, "library.renameTagFolder", json!({ "folder": my, "rename": name.trim() }), out)
    {
        ui.ctx().data_mut(|d| d.insert_temp(rename_id, name.trim().to_string()));
    }
    ui.separator();
    // A new tag inside: made from the field, moved under the folder it was made for. Its
    // colour comes from the palette beside the field — the same pick a tag's menu makes,
    // but for a tag the folder is making ([#1254] review › color options in a folder's
    // menu); a pick is remembered for the menu, a non-pick takes the named colours' next.
    let pick_id = Id::unique(("browser-folder-tag-color", epoch, folder_id));
    let picked_now = {
        let stored: Option<String> = ui.ctx().data(|d| d.get_temp(pick_id));
        let current = stored.as_deref().and_then(theme::parse_hex).map(crate::interaction::grid::to_rgb);
        crate::interaction::colour_picker::select_colour_row(ui, pick_id, current, "Select colour")
            .map(|picked| theme::hex(crate::interaction::colour_picker::color32(picked)))
    };
    if let Some(hex) = picked_now.as_ref() {
        ui.ctx().data_mut(|d| d.insert_temp(pick_id, hex.clone()));
    }
    let new_id = Id::unique(("browser-folder-new-tag", epoch, folder_id));
    let mut new_name: String = ui.ctx().data(|d| d.get_temp(new_id)).unwrap_or_default();
    let field = crate::kit::widgets::text_input(
        ui,
        egui::TextEdit::singleline(&mut new_name).id(new_id).hint_text("New tag…").desired_width(120.0),
    );
    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New tag name".to_string()));
    ui.ctx().data_mut(|d| d.insert_temp(new_id, new_name.clone()));
    let with_enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let with_button = ui.button("Add tag").clicked();
    if (with_enter || with_button) && !new_name.trim().is_empty() {
        // The name one of the tags has: refused before the command, as the section's menus
        // do ([#1254] review: `setTag` would recolour and file that tag instead, and say it
        // was added). The name a row of the destination's own level holds is the store's
        // refusal now: the make lands in the folder from its first byte, its name asked of
        // that level ([#1254] review).
        if projects.file_library_mut().tag_flat().iter().any(|t| t.name == new_name.trim()) {
            out.status = Some(Status::Error(format!("there is a tag {:?} already", new_name.trim())));
        } else {
            let mut params = json!({ "name": new_name.trim(), "folder": my });
            let picked: Option<String> = ui.ctx().data(|d| d.get_temp(pick_id));
            if let Some(hex) = picked {
                params["color"] = json!(hex);
            }
            if run(projects, "library.setTag", params, out) {
                // The add shows itself: the folder opens, however deeply it sat, as the
                // subfolder's making does ([#1254] review), and its row scrolls into view.
                tag_branch_open(projects, out, folder_id);
                reveal_added_tag(state, projects, new_name.trim());
                out.status = Some(Status::Done(format!("Added tag {}", new_name.trim())));
                ui.ctx().data_mut(|d| d.remove::<String>(new_id));
                ui.ctx().data_mut(|d| d.remove::<String>(pick_id));
                state.relist();
                ui.close();
            }
        }
    }
    if ui.button("Add folder").clicked() && add_tag_folder(state, projects, json!({ "parent": my }), out) {
        // The branch itself opens, so the new folder shows wherever it sits
        // ([#1254] review).
        tag_branch_open(projects, out, folder_id);
        state.relist();
    }
    // What moves up when the folder goes: the tags AND the folders it held ([#1254] review:
    // a folder of nothing but an empty subfolder is asked about too).
    let carried = projects.file_library_mut().folder_carried(folder_id);
    let armed = state.tag_remove_confirm.as_deref() == Some(my.as_str());
    let label =
        if armed { format!("Remove folder {}? ({} move up)", store_name, carried) } else { "Remove".to_string() };
    if ui.button(label).clicked() {
        if !armed && carried > 0 {
            state.tag_remove_confirm = Some(my);
        } else {
            if run(projects, "library.removeTagFolder", json!({ "folder": my }), out) {
                state.tag_remove_confirm = None;
                state.relist();
                ui.close();
            }
        }
    }
    if armed && ui.button("Keep").clicked() {
        state.tag_remove_confirm = None;
    }
}

/// The confirm key of a removal of the whole pick: no tag or folder id takes this form.
const PICK_REMOVE: &str = "picked";

/// The menu of a row the Shift-click pick holds, with more than one picked: removes every
/// picked tag and folder. Removing a folder keeps what it held (its tags and folders move
/// up), so a picked tag inside a picked folder is removed by its own id either way. Asks
/// first when a picked tag carries items or a picked folder holds anything.
fn picked_remove_menu(
    ui: &mut Ui,
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
    counts: &std::collections::HashMap<u32, usize>,
) {
    let picked = state.tag_selection.clone();
    let forest = projects.file_library_mut().tags();
    let tags: Vec<u32> =
        picked.iter().copied().filter(|id| suhina_library::store::tag_by_id(&forest, *id).is_some()).collect();
    let folders: Vec<u32> =
        picked.iter().copied().filter(|id| suhina_library::store::folder_by_id(&forest, *id).is_some()).collect();
    let items: usize = tags.iter().map(|id| counts.get(id).copied().unwrap_or(0)).sum();
    let moved: usize = folders.iter().map(|id| suhina_library::store::folder_carried(&forest, *id)).sum();
    let what = match (tags.len(), folders.len()) {
        (t, 0) => format!("{t} tag{}", if t == 1 { "" } else { "s" }),
        (0, f) => format!("{f} folder{}", if f == 1 { "" } else { "s" }),
        (t, f) => {
            format!("{t} tag{} and {f} folder{}", if t == 1 { "" } else { "s" }, if f == 1 { "" } else { "s" })
        }
    };
    ui.label(format!("{what} picked"));
    ui.separator();
    let armed = state.tag_remove_confirm.as_deref() == Some(PICK_REMOVE);
    let label = if armed {
        let mut parts = Vec::new();
        if items > 0 {
            parts.push(format!("{items} item{} lose a tag", if items == 1 { "" } else { "s" }));
        }
        if moved > 0 {
            parts.push(format!("{moved} move up"));
        }
        format!("Remove {what}? ({})", parts.join(", "))
    } else {
        format!("Remove {what}")
    };
    if ui.button(label).clicked() {
        if !armed && (items > 0 || moved > 0) {
            state.tag_remove_confirm = Some(PICK_REMOVE.to_string());
        } else {
            let mut removed = 0;
            for id in &tags {
                if run(projects, "library.removeTag", json!({ "id": format!("#{id}") }), out) {
                    removed += 1;
                }
            }
            for id in &folders {
                if run(projects, "library.removeTagFolder", json!({ "folder": format!("#{id}") }), out) {
                    removed += 1;
                }
            }
            // As a single removal: the reconcile drops the gone ids from the place, the
            // filter and the pick whatever the commands said ([#1254] review).
            state.reconcile(projects);
            state.tag_selection.clear();
            state.tag_remove_confirm = None;
            state.relist();
            if removed == tags.len() + folders.len() {
                out.status = Some(Status::Done(format!("Removed {what}")));
            }
            ui.close();
        }
    }
    if armed && ui.button("Keep").clicked() {
        state.tag_remove_confirm = None;
    }
}

/// The Tags section's rows: the tags and folders as places ([#1254]), which the sidebar
/// draws when the section is open.
fn tags_section(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    py: &mut f32,
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
) {
    // The tag menus' state keys on the library's epoch too: a replaced library's tag of the
    // same id opens none of the old one's menus or drafts ([#1254] review).
    let epoch = state.tag_epoch;
    let item_font = theme::ui_font(theme::TEXT_SMALL);
    let mut y = *py;
    let tag_list = projects.file_library_mut().tags();
    let counts = state.tag_counts(projects.file_library_mut());
    let mut rows = Vec::new();
    tag_rows_of(&tag_list, 0, &mut rows);

    // The pick prunes itself against the whole forest: a dead id the sidebar or the footer's
    // removal left (the places' own reconciles run where the library's changes land,
    // [#1254] review) takes nothing of the pick's landing with it. A row a closed folder
    // hides still is, and stays picked: a command can close the folder without a click
    // clearing the pick, and the group's move then takes it along ([#1254] review).
    let live: std::collections::HashSet<u32> = suhina_library::store::tag_ids(&tag_list)
        .into_iter()
        .chain(suhina_library::store::folders_iter(&tag_list).map(|f| f.id))
        .collect();
    state.tag_selection.retain(|id| live.contains(id));

    // The rows' own geometry first: rects are derived from the rows to draw, so the frame's
    // fill and the landing know them in time ([#1254] review: the drop's row lights on the
    // very frames it is dragged over). Where two rows change level a few points of air go
    // between them ([#1254] review: the nesting reads like the sidebar's groups do).
    let mut rects: Vec<Rect> = Vec::with_capacity(rows.len());
    let mut y_rows = y;
    for (index, row) in rows.iter().enumerate() {
        rects.push(Rect::from_min_size(Pos2::new(area.left(), y_rows), Vec2::new(area.width(), ROW)));
        let air = rows.get(index + 1).is_some_and(|next| next.depth != row.depth);
        y_rows += ROW + if air { LEVEL_GAP } else { 0.0 };
    }
    let row_rects: Vec<(u32, String, bool, bool, Rect)> = rows
        .iter()
        .zip(rects.iter())
        .map(|(row, rect)| {
            (
                row.entry.id(),
                row.entry.name().to_string(),
                matches!(row.entry, suhina_library::TagEntry::Folder { .. }),
                matches!(&row.entry, suhina_library::TagEntry::Folder { folder, .. } if folder.open),
                rect.intersect(area),
            )
        })
        .collect();
    y = y_rows;
    // A row just added shows itself: the sidebar's scroll moves just far enough to hold it,
    // at the end of the frame, once the room is measured ([#1254] review). A row this
    // section does not draw (a folder closed over it) asks nothing more.
    if let Some(wanted) = state.reveal_tag_row.take()
        && let Some(rect) = rows.iter().zip(rects.iter()).find(|(row, _)| row.entry.id() == wanted).map(|(_, r)| *r)
    {
        state.sidebar_reveal_by = reveal_shift(rect, area);
    }
    // Where a carried tag's drop goes, from the frame's own rows: derived before the rows
    // paint, so what the pointer is over lights that very frame ([#1254] review). A press
    // arms its carry in the painting pass; the carry made there is derived again after the
    // rows ([#1254] review: a drag's arm frame says its drop as any frame does).
    let pointer = ui.input(|i| i.pointer.latest_pos());
    // The carried row's kind: a drop before a row of the other kind cannot show in the
    // folder-first display, and the pill says where it lands instead ([#1254] review).
    let carried_folder = state.drag_tag_id.is_some_and(|carried| folder_row_is_folder(&rows, carried));
    if state.drag_tag_id.is_some() {
        state.drop_tag_row = None;
        state.drop_folder_before = None;
        state.drop_pill = None;
        for (rid, name, is_folder, open, rect) in &row_rects {
            if !pointer.is_some_and(|p| rect.contains(p)) {
                continue;
            }
            if *rid == state.drag_tag_id.unwrap_or(0) {
                state.drop_pill = state.drag_tag.clone();
            } else {
                state.drop_tag_row = Some(*rid);
                // A closed folder takes the drop into itself (the hint says so ([#1254]));
                // an open folder's lower half goes in as well, its top half orders the
                // carried one before it, as the spec's *before another row* does
                // ([#1254] review).
                let top_half = pointer.is_some_and(|p| p.y < rect.center().y) && *open;
                state.drop_folder_before = if *is_folder && top_half { Some(*rid) } else { None };
                state.drop_pill = Some(if *is_folder && !top_half {
                    format!("Move {} into {name}", state.drag_tag.clone().unwrap_or_default())
                } else if carried_folder != *is_folder {
                    if carried_folder {
                        format!("Put {} above the folders", state.drag_tag.clone().unwrap_or_default())
                    } else {
                        format!("Put {} above the tags", state.drag_tag.clone().unwrap_or_default())
                    }
                } else {
                    format!("Put {} before {name}", state.drag_tag.clone().unwrap_or_default())
                });
                if *is_folder && !*open {
                    out.status = Some(Status::Hint(format!("Drop to put it in {name}")));
                }
            }
            break;
        }
    } else {
        state.drop_folder_before = None;
    }
    for (index, row) in rows.iter().enumerate() {
        let rect = rects[index];
        let id = row.entry.id();
        let name = row.entry.name().to_string();
        let is_folder = matches!(row.entry, suhina_library::TagEntry::Folder { .. });
        let count = match &row.entry {
            suhina_library::TagEntry::Tag(tag) => counts.get(&tag.id).copied().unwrap_or(0),
            suhina_library::TagEntry::Folder { tags, .. } => suhina_library::store::tag_count(tags),
        };
        let open = match &row.entry {
            suhina_library::TagEntry::Folder { folder, .. } => folder.open,
            _ => true,
        };
        // A press carries the tag to an item of the results (and a folder's carries the
        // folder, to move it among its cortège); a click selects the tag's place and a
        // folder's row opens or closes it.
        let response =
            ui.interact(rect.intersect(area), Id::unique(("browser-tag-place", epoch, id)), Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                true,
                if is_folder {
                    format!("Folder {} ({} tag{})", name, count, if count == 1 { "" } else { "s" })
                } else {
                    format!("Place Tag {}", name)
                },
            )
        });
        // A drag of the row itself carries the tag or folder ([#1254] › Tag folders): to an
        // item row, to a folder, and to a position before a row's own.
        if response.drag_started() && rect.intersects(area) {
            state.drag_tag = Some(name.clone());
            state.drag_tag_id = Some(id);
            suhina_plugins::trace!("tag carry starts {} (id {})", name, id);
        }
        // Dragging items here tags them when they land on a tag's row; a folder's own rows
        // are never a place for the items, and a closed folder says so ([#1254]). The
        // drag's own pointer: a drag carried from the map's own window reaches here as
        // that window's pointer ([#1254] review).
        let pointer = state.drag_pointer.or_else(|| ui.input(|i| i.pointer.latest_pos()));
        let drag_here =
            state.drag.is_some() && rect.intersects(area) && pointer.is_some_and(|p| rect.intersect(area).contains(p));
        if drag_here {
            if is_folder {
                out.status = Some(Status::Hint(if open {
                    format!("Drop on a tag of {} to tag them", name)
                } else {
                    format!("Open {} to drop on its tags", name)
                }));
            } else {
                state.drop_tag = Some(name.clone());
            }
        }
        // A carried tag or folder lights its drop targets: a folder takes it in (closed
        // ones too — the drop lands inside and the hint says so), a tag's row takes it
        // before itself ([#1254] › Tag folders).
        // Where a carried tag is: derived once after the loop from the drawn rows
        // ([#1254] review), with the fill reading the frame's earlier state.
        let move_here = state.drop_tag_row == Some(id);
        let chosen = matches!(&state.place, Some(Place::Tag(tags)) if tags.contains(&id));
        let alone = state.place.as_ref() == Some(&Place::Tag(vec![id]));
        let picked = state.tag_selection.contains(&id);
        // What a drop of dragged items lands on lights as it would under the pointer
        // ([#1254] review: which tag a drop goes to shows the same highlight).
        let items_here = !is_folder && state.drag.is_some() && state.drop_tag.as_deref() == Some(name.as_str());
        let fill = if move_here || items_here {
            theme::control_hover()
        } else if chosen {
            theme::control_active()
        } else {
            motion::mix(egui::Color32::TRANSPARENT, theme::panel_hover(), widgets::hover(ui, &response))
        };
        let style = place_style(&response, chosen);
        if rect.intersects(area) {
            paint_row(painter, &style, rect.intersect(area), fill);
            // The section's rows sit a step in from its title: a top-level folder's chevron
            // under the title's name, not under the title's own chevron ([#1254] review: Tags
            // and its first folder read as one level). Each level a step further.
            let lead = Pos2::new(tag_row_lead(area.left(), row.depth), rect.center().y);
            if is_folder {
                widgets::chevron(painter, Pos2::new(lead.x + 5.0, lead.y), open, theme::browser_text_hint());
            }
            let dim = !is_folder && count == 0;
            let center = Pos2::new(lead.x + (if is_folder { 16.0 } else { 7.0 }), rect.center().y);
            if !is_folder {
                let mut color = match &row.entry {
                    suhina_library::TagEntry::Tag(tag) => tag_color(&tag.color),
                    _ => theme::text_hint(),
                };
                if dim {
                    color = color.gamma_multiply(0.5);
                }
                painter.circle_filled(center, DOT / 2.0, color);
            }
            // A picked row's ring, a tag's dot's and a folder's own beside its chevron:
            // where a group move's membership reads ([#1254] review).
            if chosen || picked {
                painter.circle_stroke(center, DOT / 2.0 + 1.5, Stroke::new(1.0, theme::browser_text_primary()));
            }
            let text = Rect::from_min_max(
                Pos2::new(
                    if is_folder {
                        // A folder's name beside its chevron, no mark before it.
                        center.x - 4.0
                    } else {
                        center.x + DOT / 2.0 + 3.0
                    },
                    rect.top(),
                ),
                Pos2::new(rect.right() - PAD, rect.bottom()),
            );
            let tinted = if dim {
                theme::browser_text_hint()
            } else if chosen || response.hovered() {
                theme::browser_text_primary()
            } else {
                theme::browser_text_secondary()
            };
            widgets::text_in(painter, text, Align2::LEFT_CENTER, &name, item_font.clone(), style.color_or(tinted));
            if response.hovered() {
                out.status = Some(Status::Hint(format!(
                    "{}{}, {} {}{}",
                    if is_folder { "Folder " } else { "Tag " },
                    name,
                    count,
                    if is_folder { "tag" } else { "item" },
                    if count == 1 { "" } else { "s" }
                )));
            }
            if !chosen {
                widgets::text_in(
                    painter,
                    text,
                    Align2::RIGHT_CENTER,
                    &count.to_string(),
                    theme::mono_font(theme::TEXT_TINY),
                    if dim { theme::browser_text_hint().gamma_multiply(0.5) } else { theme::browser_text_hint() },
                );
            }
        }
        if chosen && !is_folder && rect.intersects(area) {
            // As any chosen place: a × lets go of it.
            let clear = Rect::from_min_size(Pos2::new(rect.right() - ROW, rect.top()), Vec2::splat(ROW));
            let cleared = widgets::button_area(
                ui,
                clear.intersect(area),
                Id::unique(("browser-tag-clear", epoch, id)),
                "Show everything",
            );
            let color =
                motion::mix(theme::browser_text_hint(), theme::browser_text_primary(), widgets::hover(ui, &cleared));
            widgets::text_in(painter, clear, Align2::CENTER_CENTER, "×", item_font.clone(), color);
            if cleared.clicked() {
                state.place = None;
                state.category = Category::All;
                state.scroll = 0.0;
            }
        }
        if response.clicked() {
            let ctrl = ui.input(|i| i.modifiers).command;
            let shift = ui.input(|i| i.modifiers).shift;
            if shift {
                // Shift-click picks tags for one carry: a drag of any of them lands them
                // all in a folder ([#1254] review). A picked tag keeps its place work.
                if state.tag_selection.contains(&id) {
                    state.tag_selection.retain(|t| *t != id);
                } else {
                    state.tag_selection.push(id);
                }
            } else {
                if !state.tag_selection.is_empty() {
                    // A plain click clears the pick.
                    state.tag_selection.clear();
                }
                match &row.entry {
                    suhina_library::TagEntry::Folder { .. } => {
                        if ctrl {
                            // A folder's tags, its subfolders' under them, add to the place, or
                            // go off again ([#1254] › Tag folders).
                            let mut under = Vec::new();
                            collect_folder_tag_ids(std::slice::from_ref(&row.entry), &mut under);
                            let carried = match &state.place {
                                Some(Place::Tag(tags)) => tags,
                                _ => &[0u32; 0][..],
                            };
                            let taken = !under.iter().all(|u| carried.contains(u));
                            if taken {
                                let mut tags = match &state.place {
                                    Some(Place::Tag(tags)) => tags.clone(),
                                    _ => Vec::new(),
                                };
                                for u in under {
                                    if !tags.contains(&u) {
                                        tags.push(u);
                                    }
                                }
                                state.place = Some(Place::Tag(tags));
                            } else {
                                let mut tags = match &state.place {
                                    Some(Place::Tag(tags)) => tags.clone(),
                                    _ => Vec::new(),
                                };
                                tags.retain(|t| !under.contains(t));
                                state.place = if tags.is_empty() { None } else { Some(Place::Tag(tags)) };
                            }
                            state.category = Category::All;
                            state.scroll = 0.0;
                        } else {
                            if run(
                                projects,
                                "library.setFolderOpen",
                                json!({ "folder": format!("#{id}"), "open": !open }),
                                out,
                            ) {
                                state.relist();
                            }
                            out.status = Some(Status::Hint(if open {
                                format!("Closed {}. Click again to open it", name)
                            } else {
                                format!("Open {}. Ctrl-click tags with its tags", name)
                            }));
                        }
                    }
                    suhina_library::TagEntry::Tag(_) => {
                        if chosen && ctrl && alone {
                            state.place = None;
                        } else if chosen && ctrl {
                            // Ctrl-click takes the tag off the combined place again.
                            if let Some(Place::Tag(tags)) = &mut state.place {
                                tags.retain(|t| *t != id);
                                if tags.is_empty() {
                                    state.place = None;
                                }
                            }
                        } else if chosen && !ctrl {
                            // A plain click of a tag the combined place holds shows its tag
                            // alone: the place is the clicked tag's ([#1254] review).
                            state.place = Some(Place::Tag(vec![id]));
                            state.category = Category::All;
                            state.scroll = 0.0;
                        } else if !chosen {
                            if ctrl
                                && let Some(Place::Tag(tags)) = &mut state.place
                                && !tags.contains(&id)
                            {
                                // Ctrl-click adds the tag to the combined place: items with any of them.
                                tags.push(id);
                            } else {
                                state.place = Some(Place::Tag(vec![id]));
                            }
                            state.category = Category::All;
                            state.scroll = 0.0;
                        }
                    }
                }
            }
        }
        // Right-click: rename (as it is typed), recolour, add inside, remove — asking first
        // while a remove empties a folder ([#1254] › Tag folders).
        let was_open =
            ui.ctx().data(|d| d.get_temp::<bool>(Id::unique(("browser-row-menu-open", epoch, id)))).unwrap_or(false);
        let popup = egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                // A row the Shift-click pick holds, with others picked beside it: the menu
                // acts on the whole pick, as the results' row menu acts on the selection.
                if picked && state.tag_selection.len() > 1 {
                    picked_remove_menu(ui, state, projects, out, &counts);
                    return;
                }
                if is_folder {
                    tag_folder_menu(ui, row.entry.id(), state, projects, out);
                    return;
                }
                let field_id = Id::unique(("browser-rename-tag", epoch, id));
                let mut name: String = ui.ctx().data(|d| d.get_temp(field_id)).unwrap_or_else(|| name.clone());
                let rename_field = crate::kit::widgets::text_input(
                    ui,
                    egui::TextEdit::singleline(&mut name).id(field_id).desired_width(120.0),
                );
                rename_field.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("Rename tag {}", row.entry.name()))
                });
                if rename_field.clicked() {
                    rename_field.request_focus();
                }
                ui.ctx().data_mut(|d| d.insert_temp(field_id, name.clone()));
                // Enter commits, and so does a click outside the menu ([#1250] review: a rename
                // under every key saved the library once a key, and a name that begins as
                // another tag's was refused before the rest of it could be typed). An erased
                // field commits nothing; another tag's name is refused with the status's
                // saying, and the field resets so a dead draft does not run again.
                let committed = (rename_field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                    || ui.input(|i| {
                        i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !ui.max_rect().contains(p))
                    });
                if committed && !name.trim().is_empty() && name.trim() != row.entry.name() {
                    if run(
                        projects,
                        "library.setTag",
                        json!({ "id": format!("#{}", row.entry.id()), "rename": name.trim() }),
                        out,
                    ) {
                        ui.ctx().data_mut(|d| d.remove::<String>(field_id));
                        state.relist();
                    } else {
                        ui.ctx().data_mut(|d| d.insert_temp(field_id, row.entry.name().to_string()));
                    }
                }
                let current = theme::parse_hex(tag_row_color(&row.entry)).map(crate::interaction::grid::to_rgb);
                // Select colour: the palette's swatches and the theme editor's picker, opened in
                // the menu (a picker of its own would end the menu as a click outside it does),
                // the colour one command a pick.
                if let Some(picked) = crate::interaction::colour_picker::select_colour_row(
                    ui,
                    Id::unique(("browser-tag-colour", epoch, row.entry.id())),
                    current,
                    "Select colour",
                ) {
                    let hex = theme::hex(crate::interaction::colour_picker::color32(picked));
                    run(projects, "library.setTag", json!({ "id": format!("#{}", row.entry.id()), "color": hex }), out);
                }
                ui.separator();
                // A new tag from here: a typed name, added with Enter or the Add tag button, in
                // the colour this menu shows outlined, shown dimmed until something carries it
                // (#1196).
                let new_id = Id::unique(("browser-section-new-tag", epoch, row.entry.id()));
                let mut new_name: String = ui.ctx().data(|d| d.get_temp(new_id)).unwrap_or_default();
                let field = crate::kit::widgets::text_input(
                    ui,
                    egui::TextEdit::singleline(&mut new_name).id(new_id).hint_text("New tag…").desired_width(120.0),
                );
                field
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New tag name".to_string()));
                ui.ctx().data_mut(|d| d.insert_temp(new_id, new_name.clone()));
                let with_enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let with_button = ui.button("Add tag").clicked();
                if (with_enter || with_button) && !new_name.trim().is_empty() {
                    let taken = projects.file_library_mut().tag_flat();
                    if taken.iter().any(|t| t.name == new_name.trim()) {
                        out.status = Some(Status::Error(format!("there is a tag {:?} already", new_name.trim())));
                    } else {
                        // The colour this menu shows outlined: a new tag comes out of the colour
                        // the tag it was added from shows (#1196/#1254).
                        let color = tag_row_color(&row.entry).to_string();
                        if run(projects, "library.setTag", json!({ "name": new_name.trim(), "color": color }), out) {
                            reveal_added_tag(state, projects, new_name.trim());
                            out.status = Some(Status::Done(format!("Added tag {}", new_name.trim())));
                            ui.ctx().data_mut(|d| d.remove::<String>(new_id));
                            state.relist();
                            // Done is done: the add closes the menu, as a rename or a remove
                            // do; a refused name keeps it, so the name can be corrected.
                            ui.close();
                        }
                    }
                }
                // A new folder beside the tag: the right-click on any tag offers the first
                // folder too, at the tag's own level ([#1254] review › New folder on any tag).
                if ui.button("New folder").clicked() {
                    let store = projects.file_library_mut().tags();
                    let parent = suhina_library::store::find_entry_parent(&store, row.entry.id()).and_then(|(p, _)| p);
                    let params = match parent {
                        Some(folder) => json!({ "parent": format!("#{folder}") }),
                        None => json!({}),
                    };
                    if add_tag_folder(state, projects, params, out) {
                        // The target's branch opens, so the new folder shows wherever it
                        // landed ([#1254] review).
                        if let Some(folder) = parent {
                            tag_branch_open(projects, out, folder);
                        }
                        state.relist();
                        ui.close();
                    }
                }
                let armed = state.tag_remove_confirm.as_deref() == Some(row.entry.name());
                let label = if armed {
                    format!("Remove {}? ({count} items)", row.entry.name())
                } else {
                    format!("Remove tag {}", row.entry.name())
                };
                if ui.button(label).clicked() {
                    if !armed && suhina_library::store::tag_count(std::slice::from_ref(&row.entry)) > 0 {
                        state.tag_remove_confirm = Some(row.entry.name().to_string());
                    } else {
                        run(projects, "library.removeTag", json!({ "id": format!("#{}", row.entry.id()) }), out);
                        // The removal's purge is the reconcile's: the pick, the place and the
                        // filter lose the id the library dropped — a save that failed leaves
                        // the memory changed all the same, so the reconcile runs whatever the
                        // command said ([#1254] review).
                        state.reconcile(projects);
                        state.tag_remove_confirm = None;
                        state.relist();
                        ui.close();
                    }
                }
                if armed && ui.button("Keep").clicked() {
                    state.tag_remove_confirm = None;
                }
            });
        // Closed: the drafts go with it, as the footer's menus do ([#1254] review: a stale
        // draft would replay a refused rename every open, and save an old name over an
        // external one — one command resets the field to what the store has).
        let open = popup.is_some();
        let open_key = Id::unique(("browser-row-menu-open", epoch, id));
        // A click outside that closed the menu before its body ran commits the typed name
        // as the body would have, as the footer's menu does ([#1250] review).
        if was_open && !open && !is_folder && ui.input(|i| i.pointer.any_click()) {
            let draft: Option<String> = ui.ctx().data(|d| d.get_temp(Id::unique(("browser-rename-tag", epoch, id))));
            if let Some(typed) = draft.as_deref().map(str::trim).filter(|n| !n.is_empty() && *n != name.as_str())
                && run(projects, "library.setTag", json!({ "id": format!("#{id}"), "rename": typed }), out)
            {
                state.relist();
            }
        }
        if was_open && !open {
            // The remove's asking goes with its menu: armed and let go with, the next
            // opening would remove on a click that never asked ([#1250] review).
            state.tag_remove_confirm = None;
            ui.ctx().data_mut(|d| {
                if is_folder {
                    d.remove::<String>(Id::unique(("browser-tag-folder", epoch, id)));
                    d.remove::<String>(Id::unique(("browser-folder-new-tag", epoch, id)));
                    // The pick held for the tag it was to colour: a menu that closes
                    // without its add leaves no colour for the next one to inherit
                    // ([#1254] review).
                    d.remove::<String>(Id::unique(("browser-folder-tag-color", epoch, id)));
                    crate::interaction::colour_picker::forget_state(
                        d,
                        Id::unique(("browser-folder-tag-color", epoch, id)),
                    );
                } else {
                    d.remove::<String>(Id::unique(("browser-rename-tag", epoch, id)));
                    d.remove::<String>(Id::unique(("browser-section-new-tag", epoch, id)));
                    crate::interaction::colour_picker::forget_state(d, Id::unique(("browser-tag-colour", epoch, id)));
                }
            });
        }
        ui.ctx().data_mut(|d| d.insert_temp(open_key, open));
    }
    // The carry armed in this very pass: the pre-paint derivation ran before it, so it
    // derives now — the pointer of this frame, the rows the frame drew ([#1254] review: a
    // drag's own arm frame says its drop as the next one would).
    if state.drag_tag_id.is_some() && state.drop_pill.is_none() {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        state.drop_tag_row = None;
        state.drop_folder_before = None;
        for (rid, name, is_folder, open, rect) in &row_rects {
            if !pointer.is_some_and(|p| rect.contains(p)) {
                continue;
            }
            if *rid == state.drag_tag_id.unwrap_or(0) {
                state.drop_pill = state.drag_tag.clone();
            } else {
                state.drop_tag_row = Some(*rid);
                let top_half = pointer.is_some_and(|p| p.y < rect.center().y) && *open;
                state.drop_folder_before = if *is_folder && top_half { Some(*rid) } else { None };
                state.drop_pill = Some(if *is_folder && !top_half {
                    format!("Move {} into {name}", state.drag_tag.clone().unwrap_or_default())
                } else if carried_folder != *is_folder {
                    if carried_folder {
                        format!("Put {} above the folders", state.drag_tag.clone().unwrap_or_default())
                    } else {
                        format!("Put {} above the tags", state.drag_tag.clone().unwrap_or_default())
                    }
                } else {
                    format!("Put {} before {name}", state.drag_tag.clone().unwrap_or_default())
                });
                if *is_folder && !*open {
                    out.status = Some(Status::Hint(format!("Drop to put it in {name}")));
                }
            }
            break;
        }
    }
    // A carried tag or folder landing: where the release lands in the sidebar decides — a
    // folder takes it in at the end, a row takes it before itself, no row of its own is the
    // top level's end ([#1254] › Tag folders). The targets are the release's own: over the
    // results' rows the app's `tag_drag` carries the tag to the item instead (drops.rs), so
    // the sidebar takes the move only when the pointer is over the sidebar ([#1254] review:
    // a pointer that crossed onto an item or a row for the releasing frame commits to
    // nothing of yesterday's target). Escape in the frame the release is: the carry ends,
    // the landing takes none of it — the app's own `tag_drag` reads the key first and the
    // two would otherwise both own the frame ([#1254] review). The landing keys on the
    // released frame — a pointer's up state flickers under the hover events of a carried
    // drag ([#1254] review).
    let up = ui.input(|i| i.pointer.primary_released());
    let escaped = ui.input(|i| i.key_pressed(Key::Escape));
    if state.drag_tag_id.is_some() {
        suhina_plugins::trace::state(
            "tag carry",
            format!(
                "armed {} up {} over row {:?} over item {} pill {:?} in_area {} pointer {pointer:?}",
                state.drag_tag_id.unwrap_or(0),
                up,
                state.drop_tag_row,
                state.drop_item.is_some(),
                state.drop_pill.as_deref().unwrap_or(""),
                pointer.is_some_and(|p| area.contains(p))
            ),
        );
    }
    if up && !escaped && state.drag_tag_id.is_some() && pointer.is_some_and(|p| area.contains(p)) {
        if let Some(carried) = state.drag_tag_id {
            let carried_name = state.drag_tag.take().unwrap_or_default();
            let target: Option<u32> = state.drop_tag_row.filter(|id| *id != carried);
            // A folder's top half was where the pointer let the drop go: before the folder,
            // not into it ([#1254] review).
            let before_folder = matches!(target, Some(t) if state.drop_folder_before == Some(t));
            let into_folder = matches!(target, Some(t) if folder_row_is_folder(&rows, t)) && !before_folder;
            // The picked ones move with the carried tag, in the pick's order — into a
            // folder, or up to the top, the carried one of them ([#1254] review: the pick
            // follows only when the drag is a picked tag's). A drop before a row
            // repositions only the carried one; the pick lifts itself here, never
            // mid-position.
            let folder_of = |target: u32| -> Option<String> {
                rows.iter().find(|r| r.entry.id() == target).map(|r| r.entry.name().to_string())
            };
            let filing = target.is_none() || into_folder;
            let carrying = if filing && state.tag_selection.contains(&carried) {
                state.tag_selection.clone()
            } else {
                vec![carried]
            };
            let mut landed = 0;
            let mut landing = String::new();
            let mut refused: Option<String> = None;
            for entry_id in carrying {
                let mut params = json!({ "id": format!("#{entry_id}") });
                let did = match target {
                    Some(target) if into_folder => {
                        params["folder"] = json!(format!("#{target}"));
                        format!("Moved {} into {}", carried_name, folder_of(target).unwrap_or_default())
                    }
                    Some(target) if !filing => {
                        // The carried one comes out before the insert: where it sat in the
                        // same level ahead of the target, that target steps left one when
                        // it is taken, so the before-position follows it ([#1254] review).
                        let carried_folder = folder_row_is_folder(&rows, entry_id);
                        let target_folder = folder_row_is_folder(&rows, target);
                        let others = projects.file_library_mut().tags();
                        if let Some((t_parent, t_at)) = suhina_library::store::find_entry_parent(&others, target) {
                            if let Some(p) = t_parent {
                                params["folder"] = json!(format!("#{p}"));
                            }
                            // The display leads a level with its folders and the tags follow
                            // ([#1254] review): a row of the other kind cannot show between
                            // the target and what stands above it, so it lands before the
                            // first row of its own kind at the level instead — the position
                            // the folder-first order can show. Rows of one kind keep the
                            // true before-position.
                            let position = if carried_folder == target_folder {
                                match suhina_library::store::find_entry_parent(&others, entry_id) {
                                    Some((parent, at)) if parent == t_parent && at < t_at => t_at.saturating_sub(1),
                                    _ => t_at,
                                }
                            } else {
                                let level: &[suhina_library::TagEntry] = match t_parent {
                                    Some(p) => suhina_library::store::folder_children(&others, p).unwrap_or(&[]),
                                    None => &others,
                                };
                                match level.iter().position(|e| {
                                    matches!(e, suhina_library::TagEntry::Folder { .. }) == carried_folder
                                }) {
                                    Some(head) => match suhina_library::store::find_entry_parent(&others, entry_id) {
                                        Some((parent, at)) if parent == t_parent && at < head => head.saturating_sub(1),
                                        _ => head,
                                    },
                                    None => t_at,
                                }
                            };
                            params["position"] = json!(position);
                        }
                        if carried_folder == target_folder {
                            format!("Moved {} before {}", carried_name, folder_of(target).unwrap_or_default())
                        } else if carried_folder {
                            format!("Moved {} above the folders", carried_name)
                        } else {
                            format!("Moved {} above the tags", carried_name)
                        }
                    }
                    _ => format!("Moved {} to the top of the tags", carried_name),
                };
                if run(projects, "library.moveTag", params, out) {
                    landed += 1;
                    landing = did;
                } else {
                    // The refusal the run left in the status: a move that went halfway
                    // keeps it and says how much moved, so the user can carry the rest
                    // ([#1254] review).
                    if let Some(Status::Error(why)) = &out.status {
                        refused = Some(why.clone());
                    }
                    break;
                }
            }
            match (landed, refused.as_ref()) {
                // The run's own refusal stands; the pick stays with it.
                (0, _) => {}
                (1, None) => out.status = Some(Status::Done(landing)),
                (n, None) => out.status = Some(Status::Done(format!("Moved {n} tags together"))),
                (n, Some(why)) => {
                    out.status = Some(Status::Error(format!(
                        "Moved {}; rest refused: {why}",
                        if n == 1 { "1 tag".into() } else { format!("{n} tags") }
                    )))
                }
            }
            suhina_plugins::trace!("tag carry of {carried} lands {landed} over {target:?}");
            // The pick lifts itself with the move it rode in ([#1254] review: a carry that
            // was none of the pick moved alone, and the pick stays for its own).
            if refused.is_none() && filing && state.tag_selection.contains(&carried) {
                state.tag_selection.clear();
            }
            state.relist();
        }
        state.drag_tag_id = None;
    }
    // Over anything else the release is no move: the app's `tag_drag` ends the carry
    // itself (it tags an item when the pointer is over one, drops.rs).
    // No row is under the carried one: the drop goes to the section's top ([#1254] › Tag
    // folders), and the pill says it.
    if let (Some(carried_id), Some(carried_name)) = (state.drag_tag_id, state.drag_tag.as_deref())
        && state.drop_tag_row.is_none()
        && state.drop_pill.is_none()
    {
        let _ = carried_id;
        state.drop_pill = Some(format!("Move {carried_name} to the top of the tags"));
    }
    // The carried-tag's tag-of-the-row path lives in the app's own `tag_drag` (drops.rs).
    *py = y;
}

/// A section's title ([#1254] › Sidebar sections): the chevron opens and closes it by
/// command, and its context menu opens, closes and reorders — Folders cannot move, and
/// is not one of these.
#[allow(clippy::too_many_arguments)]
fn section_title(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    y: &mut f32,
    kind: SectionKind,
    open: bool,
    at: usize,
    sections_list: Vec<suhina_library::Section>,
    title_font: egui::FontId,
    slots: &mut Vec<(Rect, SectionKind)>,
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
) {
    // The title's menu keys on the library's epoch as the rows' menus do: a replaced library
    // gets none of the old one's open menu, typed name or picked colour ([#1254] review).
    let epoch = state.tag_epoch;
    *y += 8.0;
    let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, *y), Vec2::new(area.width() - PAD, 12.0));
    let response =
        ui.interact(rect.intersect(area), Id::unique(("browser-section", epoch, kind.name())), Sense::click_and_drag());
    // The slot the carry lands in: this title's own rect tells the sidebar's landing which
    // title the pointer is over ([#1254] review).
    slots.push((rect.intersect(area), kind));
    if response.drag_started() {
        // A drag of the title carries it among the others ([#1254] review).
        state.drag_section = Some(kind);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} section", kind.title())));
    widgets::chevron(
        painter,
        Pos2::new(title_chevron_x(area.left()), rect.center().y),
        open,
        theme::browser_text_hint(),
    );
    let text_rect =
        Rect::from_min_max(Pos2::new(area.left() + PAD + 14.0, rect.top()), Pos2::new(rect.right(), rect.bottom()));
    let carried = state.drag_section == Some(kind);
    widgets::text_in(
        painter,
        text_rect,
        Align2::LEFT_CENTER,
        kind.title(),
        title_font,
        if carried { theme::browser_text_hint().gamma_multiply(0.5) } else { theme::browser_text_hint() },
    );
    *y += 15.0;
    if response.clicked()
        && run(projects, "library.setSectionOpen", json!({ "section": kind.name(), "open": !open }), out)
    {
        state.relist();
    }
    // The menu: open or close, and move among the others ([#1254] › Sidebar sections). The
    // drafts inside go when it closes, so the was-open state is kept like the rows' menus'
    // (a stale draft would show an old name over what the store changed elsewhere).
    let was_open = ui
        .ctx()
        .data(|d| d.get_temp::<bool>(Id::unique(("browser-title-menu-open", epoch, kind.name()))))
        .unwrap_or(false);
    let popup =
        egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let label = if open { format!("Close {}", kind.title()) } else { format!("Open {}", kind.title()) };
            if ui.button(label).clicked()
                && run(projects, "library.setSectionOpen", json!({ "section": kind.name(), "open": !open }), out)
            {
                state.relist();
                ui.close();
            }
            let order = sections_list.iter().map(|s| s.kind).collect::<Vec<SectionKind>>();
            let (up, down) = (at > 0, at + 1 < order.len());
            if ui.add_enabled(up, egui::Button::new("Move up")).clicked() {
                let mut moved = order.clone();
                moved.swap(at - 1, at);
                let params = json!({ "order": moved.iter().map(|k| json!(k.name())).collect::<Vec<_>>() });
                if run(projects, "library.setSections", params, out) {
                    state.relist();
                    ui.close();
                }
            }
            if ui.add_enabled(down, egui::Button::new("Move down")).clicked() {
                let mut moved = order.clone();
                moved.swap(at, at + 1);
                let params = json!({ "order": moved.iter().map(|k| json!(k.name())).collect::<Vec<_>>() });
                if run(projects, "library.setSections", params, out) {
                    state.relist();
                    ui.close();
                }
            }
            // The Tags title makes tags of its own: a pick from the palette colours it, a typo
            // of a name is refused the rows' menus' way, and no pick takes the named colours'
            // next ([#1254] review: the top level has no tag to inherit a colour from, so the
            // store's way is the menu's too). The field and the button are the same the rows'
            // menus carry.
            if kind == SectionKind::Tags {
                ui.separator();
                let pick_id = Id::unique(("browser-title-tag-color", epoch, kind.name()));
                let picked_now = {
                    let stored: Option<String> = ui.ctx().data(|d| d.get_temp(pick_id));
                    let current = stored.as_deref().and_then(theme::parse_hex).map(crate::interaction::grid::to_rgb);
                    crate::interaction::colour_picker::select_colour_row(ui, pick_id, current, "Select colour")
                        .map(|picked| theme::hex(crate::interaction::colour_picker::color32(picked)))
                };
                if let Some(hex) = picked_now.as_ref() {
                    ui.ctx().data_mut(|d| d.insert_temp(pick_id, hex.clone()));
                }
                let new_id = Id::unique(("browser-title-new-tag", epoch, kind.name()));
                let mut new_name: String = ui.ctx().data(|d| d.get_temp(new_id)).unwrap_or_default();
                let field = crate::kit::widgets::text_input(
                    ui,
                    egui::TextEdit::singleline(&mut new_name).id(new_id).hint_text("New tag…").desired_width(120.0),
                );
                field
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New tag name".to_string()));
                ui.ctx().data_mut(|d| d.insert_temp(new_id, new_name.clone()));
                let with_enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let with_button = ui.button("Add tag").clicked();
                if (with_enter || with_button) && !new_name.trim().is_empty() {
                    // The name one of the tags has: refused before the command, as the rows'
                    // menus do ([#1254] review: `setTag` would recolour that tag instead and say
                    // it was added). A folder's name the store refuses itself, and the status
                    // says where it stands.
                    if projects.file_library_mut().tag_flat().iter().any(|t| t.name == new_name.trim()) {
                        out.status = Some(Status::Error(format!("there is a tag {:?} already", new_name.trim())));
                    } else {
                        let mut params = json!({ "name": new_name.trim() });
                        let picked: Option<String> = ui.ctx().data(|d| d.get_temp(pick_id));
                        if let Some(hex) = picked {
                            params["color"] = json!(hex);
                        }
                        if run(projects, "library.setTag", params, out) {
                            // The add shows itself ([#1254] review): a closed section opens,
                            // the way a folder's add opens the branch it lands in, and the
                            // row scrolls into view however many rows come before it.
                            if !open {
                                open_tags_section(projects, out);
                            }
                            reveal_added_tag(state, projects, new_name.trim());
                            out.status = Some(Status::Done(format!("Added tag {}", new_name.trim())));
                            ui.ctx().data_mut(|d| d.remove::<String>(new_id));
                            ui.ctx().data_mut(|d| d.remove::<String>(pick_id));
                            state.relist();
                            // Done is done: the menus close on their adds, a refusal keeps them
                            // open for the name to be corrected.
                            ui.close();
                        }
                    }
                }
                // The first folder still comes from here ([#1254] › Tag folders): a top-level
                // one has no row's menu to come from. It shows itself as the tag's add does: a
                // closed section opens for it ([#1254] review).
                if ui.button("New folder").clicked() && add_tag_folder(state, projects, json!({}), out) {
                    if !open {
                        open_tags_section(projects, out);
                    }
                    state.relist();
                    ui.close();
                }
            }
        });
    // Closed: the drafts go with it, as the rows' menus do ([#1254] review — a stale pick
    // or name would colour a later add without the menu showing why).
    let open = popup.is_some();
    if was_open && !open && kind == SectionKind::Tags {
        ui.ctx().data_mut(|d| {
            d.remove::<String>(Id::unique(("browser-title-new-tag", epoch, kind.name())));
            // The pick held for the tag it was to colour ([#1254] review: a menu that
            // closes without its add leaves no colour for the next one to inherit).
            d.remove::<String>(Id::unique(("browser-title-tag-color", epoch, kind.name())));
            crate::interaction::colour_picker::forget_state(
                d,
                Id::unique(("browser-title-tag-color", epoch, kind.name())),
            );
        });
    }
    ui.ctx().data_mut(|d| d.insert_temp(Id::unique(("browser-title-menu-open", epoch, kind.name())), open));
}

/// Opens the Tags section, for an add made while it was closed to show itself ([#1254]
/// review).
fn open_tags_section(projects: &mut Projects, out: &mut BrowserOut) {
    run(projects, "library.setSectionOpen", json!({ "section": SectionKind::Tags.name(), "open": true }), out);
}

/// Where a section title's chevron is drawn, its centre's x, in a sidebar whose left is `left`.
fn title_chevron_x(left: f32) -> f32 {
    left + PAD + 4.0
}

/// Where a Tags section row's lead starts at `depth` (0 for the top level): a step in from the
/// section's title, a folder's chevron 5 points after it and a tag's dot 7 ([#1254] review).
fn tag_row_lead(left: f32, depth: usize) -> f32 {
    left + PAD + TAGS_LEAD + depth as f32 * INDENT
}

/// A subfolder's making opens the branch from its target up ([#1254] review): the new row
/// shows however deeply the target sat, and the branch keeps its own leaves.
fn tag_branch_open(projects: &mut Projects, out: &mut BrowserOut, folder_id: u32) {
    let mut chain = Vec::new();
    {
        let store = projects.file_library_mut().tags();
        let mut cursor = Some(folder_id);
        while let Some(id) = cursor {
            chain.push(id);
            cursor = suhina_library::store::find_entry_parent(&store, id).and_then(|(p, _)| p);
        }
    }
    for id in chain {
        run(projects, "library.setFolderOpen", json!({ "folder": format!("#{id}"), "open": true }), out);
    }
}

fn row_menu(
    ui: &mut Ui,
    item: &BrowserItem,
    tags: &[suhina_library::Tag],
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
) {
    if item.kind == ItemKind::Heading {
        return;
    }
    let epoch = state.tag_epoch;
    // The row's own item, or every selected one when the row is among them: Pin, Unpin and Tag
    // act on the selection as Copy to folder does ([#1254] review: unpinning six selected
    // samples unpinned the one right-clicked).
    let targets = menu_targets(item, state.selected());
    let several = targets.len() > 1;
    let pinned = projects.file_library_mut().pinned().contains(&item.key);
    let pin_label = match (pinned, several) {
        (true, false) => "Unpin".to_string(),
        (false, false) => "Pin".to_string(),
        (true, true) => format!("Unpin {} items", targets.len()),
        (false, true) => format!("Pin {} items", targets.len()),
    };
    if ui.button(pin_label).clicked() {
        let command = if pinned { "library.unpin" } else { "library.pin" };
        for key in &targets {
            run(projects, command, json!({ "item": key }), out);
        }
        state.relist();
        ui.close();
    }
    // A tag shows ticked when every target carries it; a click sets it on all of them, or
    // clears it from all when all carry it.
    let carried: Vec<Vec<u32>> = targets.iter().map(|key| projects.file_library_mut().item_tags(key)).collect();
    ui.menu_button("Tag", |ui| {
        // A library wide in tags hides rows past the window: the rows scroll inside their
        // own room, as the footer's overflow does ([#1254] review), and the field below
        // stays fixed at the menu's end.
        egui::ScrollArea::vertical().max_height(OVERFLOW_ROWS * OVERFLOW_ROW).show(ui, |ui| {
            for tag in tags {
                let on = carried.iter().all(|tags| tags.contains(&tag.id));
                if tag_entry(ui, tag, on).clicked() {
                    for key in &targets {
                        run(
                            projects,
                            "library.tag",
                            json!({ "item": key, "tag": format!("#{}", tag.id), "on": !on }),
                            out,
                        );
                    }
                    state.relist();
                    ui.close();
                }
            }
        });
        ui.separator();
        // A new tag, set on the targets at once.
        // Keyed on the library's epoch: a replaced library's menu starts with no draft of the
        // old one's to make there ([#1254] review).
        let id = Id::unique(("browser-new-tag", epoch));
        let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let response = crate::kit::widgets::text_input(
            ui,
            egui::TextEdit::singleline(&mut name).hint_text("New tag…").desired_width(120.0),
        );
        ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !name.trim().is_empty() {
            // The named colours' next turn colours it ([#1254]: the store hands the default
            // colour out when the command is asked without one).
            let name = name.trim().to_string();
            if run(projects, "library.setTag", json!({ "name": name }), out) {
                for key in &targets {
                    run(projects, "library.tag", json!({ "item": key, "tag": name, "on": true }), out);
                }
            }
            ui.ctx().data_mut(|d| d.remove::<String>(id));
            state.relist();
            ui.close();
        }
    });
    if let Some(Item::File(path)) = item.item()
        && let Some(parent) = path.parent()
        && item.kind != ItemKind::Folder
        && ui.button("Show in folder").clicked()
    {
        state.search.clear();
        state.place = Some(Place::Folder(parent.to_path_buf()));
        ui.close();
    }
    // The row's file, or every selected file when the row is one of them, copied into a
    // folder; the originals stay (#865).
    let files = copy_files(item, state.selected());
    if !files.is_empty() && ui.button("Copy to folder…").clicked() {
        out.copy_to = files;
        ui.close();
    }
    if item.kind == ItemKind::Folder && ui.button("Refresh").clicked() {
        state.relist();
        ui.close();
    }
}

/// The files Copy to folder… copies from `item`'s menu: every selected sample when `item` is one
/// of the selection, else `item` alone, when it is a sample.
/// The items a row's Pin, Unpin and Tag act on: every selected item (headings aside) when the
/// row is one of them, else the row's own ([#1254] review).
fn menu_targets(item: &BrowserItem, selected: &[BrowserItem]) -> Vec<String> {
    if selected.iter().any(|s| s.key == item.key) {
        selected.iter().filter(|s| s.kind != ItemKind::Heading).map(|s| s.key.clone()).collect()
    } else {
        vec![item.key.clone()]
    }
}

pub fn copy_files(item: &BrowserItem, selected: &[BrowserItem]) -> Vec<PathBuf> {
    let sample = |i: &BrowserItem| (i.kind == ItemKind::Sample).then(|| i.path()).flatten();
    // Only a sample's menu copies: a MIDI file's would otherwise copy the samples picked with it.
    if item.kind != ItemKind::Sample {
        return Vec::new();
    }
    if selected.iter().any(|s| s.key == item.key) {
        selected.iter().filter_map(sample).collect()
    } else {
        sample(item).into_iter().collect()
    }
}

/// One entry of an item's Tag menu: a box, ticked while the item carries the tag, a swatch in
/// the tag's colour and its name. The box and the swatch are painted, not text, so no font
/// has to hold a check mark (#853). A click anywhere on the row toggles the tag.
fn tag_entry(ui: &mut Ui, tag: &suhina_library::Tag, on: bool) -> egui::Response {
    const BOX: f32 = 12.0;
    const SWATCH: f32 = 8.0;
    let font = theme::ui_font(theme::TEXT);
    let text_left = 6.0 + BOX + 6.0 + SWATCH + 6.0;
    // As wide as its name needs while the menu measures itself, then as wide as the menu, so
    // the hover fills the row.
    let needed = text_left + widgets::text_width(ui, &tag.name, font.clone()) + 8.0;
    let width = if ui.is_sizing_pass() { needed } else { needed.max(ui.available_width()) };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ui.spacing().interact_size.y), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &tag.name));
    let painter = ui.painter();
    let hover = widgets::hover(ui, &response);
    if hover > 0.0 {
        painter.rect_filled(rect, 3.0, theme::panel_hover().gamma_multiply(hover));
    }
    let box_rect = Rect::from_min_size(Pos2::new(rect.left() + 6.0, rect.center().y - BOX / 2.0), Vec2::splat(BOX));
    painter.rect_filled(box_rect, 3.0, if on { theme::text_value() } else { theme::control_background() });
    if on {
        let r = box_rect.shrink(2.5);
        let tick = [
            Pos2::new(r.left(), r.center().y),
            Pos2::new(r.left() + r.width() * 0.4, r.bottom()),
            Pos2::new(r.right(), r.top()),
        ];
        painter.add(egui::Shape::line(
            tick.to_vec(),
            Stroke::new(1.5, crate::kit::style_class::surface_or(theme::window_background())),
        ));
    }
    let swatch = Pos2::new(box_rect.right() + 6.0 + SWATCH / 2.0, rect.center().y);
    painter.circle_filled(swatch, SWATCH / 2.0, tag_color(&tag.color));
    let text_rect = Rect::from_min_max(Pos2::new(rect.left() + text_left, rect.top()), rect.right_bottom());
    let color = motion::mix(theme::text_value(), theme::text_primary(), hover);
    widgets::text_in(painter, text_rect, Align2::LEFT_CENTER, &tag.name, font, color);
    response
}

fn start_preview(
    state: &mut BrowserState,
    projects: &mut Projects,
    settings: &Settings,
    track: Option<TrackId>,
    item: &BrowserItem,
    from: f64,
    out: &mut BrowserOut,
) {
    let Some(path) = item.path() else { return };
    if let Err(why) = can_preview(projects) {
        out.status = Some(Status::Hint(why));
        return;
    }
    match item.kind {
        ItemKind::Sample => preview_sample(state, projects, path, from),
        ItemKind::Midi => {
            state.cancel_preview();
            let mut params =
                json!({ "path": path.display().to_string(), "gain": settings.preview_volume, "from": from });
            if let Some(track) = track {
                params["track"] = json!(track.0);
            }
            if run(projects, "preview.play", params, out) {
                state.preview_started(item.key.clone());
            }
        }
        _ => {}
    }
}

/// The Instruments | Effects switch above the Plug-ins category's list; the choice is a library
/// command, kept for the next start ([the plug-in browsing spec](../../docs/spec/plugin-browsing.md)).
fn plugins_view_switch(
    ui: &mut Ui,
    area: Rect,
    projects: &mut Projects,
    chosen: suhina_library::store::PluginsView,
    out: &mut BrowserOut,
) {
    let painter = ui.painter_at(area);
    painter.hline(area.x_range(), area.bottom() - 0.5, theme::hairline(theme::border()));
    let small = theme::ui_font(theme::TEXT_SMALL);
    let mut x = area.left() + PAD;
    for view in suhina_library::store::PluginsView::ALL {
        let name = view.title();
        let width = widgets::text_width(ui, name, small.clone()) + FILTER_PAD * 2.0;
        let rect = Rect::from_min_size(Pos2::new(x, area.top() + 2.0), Vec2::new(width, FILTER));
        let response = widgets::button_area(
            ui,
            rect,
            Id::unique(("browser-plugins-view", view.name())),
            format!("Show plug-in {}", name.to_lowercase()),
        );
        if response.clicked() && chosen != view {
            run(projects, "library.setPluginsView", json!({ "view": view.name() }), out);
        }
        let style = BUTTON.response(&response).attr(Checked, chosen == view).enter();
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        widgets::button_surface(
            &painter,
            rect,
            &style,
            &response,
            chosen == view,
            if chosen == view { theme::control_selected() } else { idle },
            0.0,
        );
        let text = style.color_or(if chosen == view { theme::text_inverse() } else { theme::browser_text_secondary() });
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, name, theme::ui_font(theme::TEXT_SMALL), text);
        x = rect.right() + FILTER_GAP;
    }
}

/// The plug-ins' list's columns: the name (0), which takes what is left, then the others from
/// the right edge, with these widths.
const COLUMN_WIDTHS: [f32; 2] = [0.0, 52.0];
/// The columns' headings, by index.
const COLUMN_NAMES: [&str; 2] = ["Name", "Format"];

/// How many columns besides the name fit in `area`: those that leave the name at least
/// [`NAME_LEAST`], counted from the left, so the rightmost drop first as the panel narrows
/// (whichever side the panel is on).
fn columns_shown(area: Rect) -> usize {
    let room = area.width() - 2.0 * PAD;
    let mut shown = COLUMN_WIDTHS.len() - 1;
    while shown > 0 && room - COLUMN_WIDTHS[1..=shown].iter().sum::<f32>() < NAME_LEAST {
        shown -= 1;
    }
    shown
}

/// The left and right edge of the list's column `index` (0 name, 1 format), or `None` when it
/// has dropped for want of room.
fn column_span(area: Rect, index: usize) -> Option<(f32, f32)> {
    let shown = columns_shown(area);
    if index > shown {
        return None;
    }
    let right = area.right() - PAD - COLUMN_WIDTHS.get(index + 1..=shown).map_or(0.0, |w| w.iter().sum::<f32>());
    match index {
        0 => Some((area.left() + PAD, area.right() - PAD - COLUMN_WIDTHS[1..=shown].iter().sum::<f32>())),
        _ => Some((right - COLUMN_WIDTHS.get(index).copied().unwrap_or(0.0), right)),
    }
}

/// The list's column headings; clicking one sorts by it, again for the other direction.
fn columns_header(ui: &mut Ui, area: Rect, state: &mut BrowserState) {
    let painter = ui.painter_at(area);
    painter.hline(area.x_range(), area.bottom() - 0.5, theme::hairline(theme::border()));
    let small = theme::ui_font(theme::TEXT_SMALL);
    let (sorted, descending) = state.plugin_sort;
    for (index, name) in COLUMN_NAMES.into_iter().enumerate() {
        let Some((left, right)) = column_span(area, index) else { continue };
        let rect = Rect::from_min_size(Pos2::new(left, area.top() + 2.0), Vec2::new(right - left, FILTER));
        let response = widgets::button_area(ui, rect, Id::unique(("browser-column", index)), format!("Sort by {name}"));
        if response.clicked() {
            state.plugin_sort = (index as u8, if sorted == index as u8 { !descending } else { false });
        }
        let chosen = sorted == index as u8;
        let label =
            if chosen { format!("{name} {}", if descending { "▼" } else { "▲" }) } else { name.to_string() };
        let color = if chosen { theme::browser_text_primary() } else { theme::browser_text_hint() };
        let hover = motion::mix(egui::Color32::TRANSPARENT, theme::panel_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, hover);
        let align = if index == 0 { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER };
        widgets::text_in(&painter, rect, align, &label, small.clone(), color);
    }
}

/// Chooses `category` from the category row: the list shows again in place of the sample map,
/// and a recent place of another kind is let go, since the category would hide all it lists.
/// The category already chosen, clicked again, shows its whole tree (lets go of any place).
pub fn choose_category(state: &mut BrowserState, category: Category) {
    let again = state.category == category && state.mode == crate::browser::ResultsMode::List;
    state.mode = crate::browser::ResultsMode::List;
    let conflicts = state.place.as_ref().and_then(Place::category).is_some_and(|c| c != category);
    if again || (conflicts && category != Category::All) {
        state.place = None;
    }
    state.category = category;
}

/// Why nothing can preview now, if so: previews play through the active project's audio output,
/// or with no project open through an output of their own.
pub fn can_preview(projects: &Projects) -> Result<(), String> {
    match active(projects) {
        None if projects.audio_system().is_none() => Err("Cannot preview: there is no audio output here".into()),
        None => Ok(()),
        Some(s) if s.player().is_none() => {
            Err("Cannot preview: the project has no audio output (see the warnings when it opened)".into())
        }
        Some(_) => Ok(()),
    }
}

/// Previews a sample from `from` seconds: it is decoded on the browser's reader thread, then
/// played (`preview.play`) when it is ready. The caller checks [`can_preview`] first.
pub fn preview_sample(state: &mut BrowserState, projects: &mut Projects, path: PathBuf, from: f64) {
    state.previewing = None;
    let _ = projects.execute("preview.stop", &json!({}));
    let _ = state.info(&path, true);
    state.want_preview(path, from);
}

/// Stops any preview: the one waiting to be read, and whatever the preview voice plays, even
/// when the browser has lost track of it (#836).
pub fn stop_preview(state: &mut BrowserState, projects: &mut Projects) {
    let was = state.previewing.is_some() || state.preview_pending() || preview_sounding(projects).0;
    state.cancel_preview();
    if was {
        let _ = projects.execute("preview.stop", &json!({}));
    }
}

/// Whether the preview voice plays now.
/// Whether the preview voice plays now, and how many previews it has started.
fn preview_sounding(projects: &Projects) -> (bool, u64) {
    projects
        .preview_session()
        .and_then(|s| s.player())
        .map_or((false, 0), |p| (p.preview_monitor().playing(), p.preview_monitor().started()))
}

fn preview_strip(
    ui: &mut Ui,
    area: Rect,
    state: &mut BrowserState,
    projects: &mut Projects,
    settings: &Settings,
    track: Option<TrackId>,
    out: &mut BrowserOut,
) {
    let painter = ui.painter_at(area);
    // Without an authored surface the strip is the window's colour under the browser's panel; with
    // one, the surface `show` painted already is its background, and painting it again would
    // stack its alpha.
    if !crate::kit::style_class::has_surface() {
        painter.rect_filled(area, 0.0, theme::window_background());
    }
    painter.hline(area.x_range(), area.top() + 0.5, theme::hairline(theme::border()));
    // The Auto switch at the top right: a selected sample plays by itself while it is lit
    // (Settings › Browser › the same setting).
    let auto = Rect::from_min_size(Pos2::new(area.right() - PAD - 40.0, area.top() + 4.0), Vec2::new(40.0, 16.0));
    let response = ui
        .interact(auto, Id::unique("browser-preview-auto"), Sense::click())
        .on_hover_text("Auto preview: a sample plays when it is selected");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::CheckBox, true, "Auto preview"));
    let auto_style = BUTTON.response(&response).attr(Checked, settings.preview_autoplay).enter();
    if response.clicked() {
        out.toggle_autoplay = true;
    }
    let (fill, text) = widgets::toggle_colors(ui, &response, settings.preview_autoplay);
    widgets::button_surface(&painter, auto, &auto_style, &response, settings.preview_autoplay, fill, 3.0);
    widgets::text_in(
        &painter,
        auto,
        Align2::CENTER_CENTER,
        "Auto",
        theme::ui_font(theme::TEXT_SMALL),
        auto_style.color_or(text),
    );
    drop(auto_style);
    let area = Rect::from_min_max(area.min, Pos2::new(auto.left() - PAD, area.bottom()));
    let item = match state.selected.as_slice() {
        [one] if matches!(one.kind, ItemKind::Sample | ItemKind::Midi) => one.clone(),
        _ => {
            // Nothing to preview picked, but a preview still sounding (its point was unpicked on
            // the map): a Stop button for it, so a sounding preview always has one (#836).
            let (sounding, _) = preview_sounding(projects);
            let mut hint = area.shrink(PAD);
            if sounding {
                let stop = Rect::from_min_size(area.min + Vec2::new(PAD, 4.0), Vec2::splat(PLAY));
                let response = widgets::button_area(ui, stop, Id::unique("browser-preview-play"), "Stop preview");
                {
                    let el = BUTTON.response(&response).attr(Checked, true).enter();
                    widgets::button_surface(&painter, stop, &el, &response, true, theme::control_background(), 0.0);
                }
                play_glyph(&painter, stop.center(), true, theme::accent(), 7.0);
                if response.clicked() {
                    stop_preview(state, projects);
                }
                hint = Rect::from_min_max(Pos2::new(stop.right() + PAD, hint.top()), hint.max);
            }
            widgets::text_in(
                &painter,
                hint,
                Align2::LEFT_CENTER,
                "Select a sample or MIDI file to preview",
                theme::ui_font(theme::TEXT_SMALL),
                theme::browser_text_hint(),
            );
            return;
        }
    };
    let Some(path) = item.path() else { return };
    let monitor = projects
        .preview_session()
        .and_then(|s| s.player())
        .map(|p| (p.preview_monitor().playing(), p.preview_monitor().seconds()));
    // Stop shows while this item previews or waits to, and while any preview sounds, so a
    // sounding preview always has a control that stops it (#836).
    let playing_this = state.preview_active_for(&item.key) || monitor.is_some_and(|m| m.0);
    let info = state.info(&path, true).cloned().unwrap_or_default();

    // Top line: play, name, length, tempo, key.
    let top = Rect::from_min_size(area.min + Vec2::new(PAD, 4.0), Vec2::new(area.width() - PAD * 2.0, PLAY));
    let play = Rect::from_min_size(top.min, Vec2::splat(PLAY));
    let response = widgets::button_area(ui, play, Id::unique("browser-preview-play"), "Preview");
    let play_style = BUTTON.response(&response).attr(Checked, playing_this).enter();
    widgets::button_surface(
        &painter,
        play,
        &play_style,
        &response,
        playing_this,
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        0.0,
    );
    let ink = play_style.color_or(if playing_this { theme::accent() } else { theme::browser_text_primary() });
    if !crate::kit::skin::paint(
        &painter,
        play,
        if playing_this { "stop" } else { "play" },
        crate::kit::skin::State::response(&response, playing_this),
        ink,
        0.0,
    ) {
        play_glyph(&painter, play.center(), playing_this, ink, 7.0);
    }
    drop(play_style);
    if response.clicked() {
        if playing_this {
            stop_preview(state, projects);
        } else {
            start_preview(state, projects, settings, track, &item, 0.0, out);
        }
    }
    let mono = theme::mono_font(theme::TEXT_TINY);
    let mut facts = Vec::new();
    if let Some(seconds) = info.seconds {
        facts.push(duration_text(seconds));
    }
    if let Some(tempo) = info.tempo {
        let mark = if tempo.source == suhina_media::info::TempoSource::Detected { "~" } else { "" };
        facts.push(format!("{mark}{} BPM", (tempo.bpm * 100.0).round() / 100.0));
    }
    if let Some(key) = &info.key {
        facts.push(key.clone());
    }
    let facts = facts.join("  ");
    let facts_width = widgets::text_width(ui, &facts, mono.clone());
    let text_rect = Rect::from_min_max(Pos2::new(play.right() + PAD, top.top()), top.max);
    widgets::text_in(&painter, text_rect, Align2::RIGHT_CENTER, &facts, mono, theme::browser_text_secondary());
    let name_rect =
        Rect::from_min_max(text_rect.min, Pos2::new(text_rect.right() - facts_width - PAD, text_rect.bottom()));
    widgets::text_in(
        &painter,
        name_rect,
        Align2::LEFT_CENTER,
        &item.name,
        theme::ui_font(theme::TEXT_SMALL),
        theme::browser_text_primary(),
    );

    // The waveform, or the notes, with the part played brighter and the playhead.
    let wave = Rect::from_min_max(
        Pos2::new(area.left() + PAD, play.bottom() + 4.0),
        Pos2::new(area.right() - PAD, area.bottom() - 4.0),
    );
    let total = info.seconds.unwrap_or(0.0);
    let played = if playing_this { monitor.map_or(0.0, |m| m.1) } else { 0.0 };
    let played_x =
        if total > 0.0 { wave.left() + (played / total).clamp(0.0, 1.0) as f32 * wave.width() } else { wave.left() };
    if let Some(error) = &info.error {
        widgets::text_in(
            &painter,
            wave,
            Align2::LEFT_CENTER,
            &format!("Cannot preview: {error}"),
            theme::ui_font(theme::TEXT_SMALL),
            theme::recording(),
        );
        return;
    }
    if let Some(peaks) = &info.peaks {
        // The whole file across the width, the part played in the brighter colour.
        let count = wave.width().max(1.0) as usize;
        let per_point = if count > 0 { total / count as f64 } else { 0.0 };
        let columns = crate::kit::waveform::steady_columns(wave.left(), count, 0.0, per_point, 1.0);
        let shape = crate::kit::waveform::geometry(peaks, wave, &columns, crate::kit::waveform::scale(ui.ctx()));
        let (done, rest) = (
            Rect::from_min_max(wave.min, Pos2::new(played_x, wave.bottom())),
            Rect::from_min_max(Pos2::new(played_x, wave.top()), wave.max),
        );
        let clipped = |part: Rect| painter.with_clip_rect(part.intersect(painter.clip_rect()));
        // `browser-preview-wave { color }` for the part played, `border-color` for the rest.
        let wave = crate::kit::element::BROWSER_PREVIEW_WAVE.enter();
        let style = wave.style();
        let played = wave.color_or(theme::browser_text_secondary());
        let ahead = style.as_ref().filter(|s| s.authored.border).map_or(theme::browser_text_hint(), |s| s.border);
        crate::kit::waveform::paint(&clipped(done), &shape, crate::kit::waveform::Colors::from(played));
        crate::kit::waveform::paint(&clipped(rest), &shape, crate::kit::waveform::Colors::from(ahead));
    } else if let Some(notes) = &info.notes {
        let (low, high) = notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.2), h.max(n.2)));
        let span = f32::from(high.saturating_sub(low).max(1));
        let beats = info.beats.max(1.0);
        for (start, length, key) in notes.iter() {
            let x0 = wave.left() + (*start / beats) as f32 * wave.width();
            let x1 = wave.left() + ((start + length) / beats) as f32 * wave.width();
            let y = wave.bottom() - f32::from(key - low) / span * (wave.height() - 2.0) - 2.0;
            let color = if x0 <= played_x { theme::browser_text_secondary() } else { theme::browser_text_hint() };
            painter.rect_filled(Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.0), y + 2.0)), 0.0, color);
        }
    } else {
        widgets::text_in(
            &painter,
            wave,
            Align2::LEFT_CENTER,
            "Reading…",
            theme::ui_font(theme::TEXT_SMALL),
            theme::browser_text_hint(),
        );
    }
    if playing_this {
        painter.vline(played_x, wave.y_range(), Stroke::new(1.0, theme::accent()));
    }
    // A click on the waveform previews from there.
    let response =
        ui.interact(wave, Id::unique("browser-preview-wave"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    if response.clicked()
        && total > 0.0
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let from = f64::from((pointer.x - wave.left()) / wave.width().max(1.0)) * total;
        start_preview(state, projects, settings, track, &item, from.max(0.0), out);
    }
}

fn footer_view(ui: &mut Ui, area: Rect, state: &mut BrowserState, projects: &mut Projects, out: &mut BrowserOut) {
    // The tag menus' state keys on the library's epoch too: a replaced library's tag of the
    // same id opens none of the old one's menus or drafts ([#1254] review).
    let epoch = state.tag_epoch;
    // On the browser's own background, which `show` painted.
    let painter = ui.painter_at(area);
    painter.hline(area.x_range(), area.top() + 0.5, theme::hairline(theme::border()));
    let mut x = area.left() + PAD;
    let tags = projects.file_library_mut().tag_flat();
    // The dots' ids ride the tag's id, so a live rename keeps the menu and its field
    // ([#1254] › Tag model: the ids are the stable reference).
    for (at, tag) in tags.iter().enumerate() {
        let rect = Rect::from_center_size(Pos2::new(x + DOT / 2.0 + 2.0, area.center().y), Vec2::splat(DOT + 4.0));
        // The dots stop before the count at the right end: the tags past them keep their
        // filters through the control that takes their room ([#1254] review: a hidden one
        // lights its ring while its filter is on, and its menu reaches every filter the
        // footer hides).
        if rect.right() + 2.0 > area.right() - 110.0 {
            // Every tag from here is off the footer's dots — a last one included: an
            // active filter with no control at all would be worse than a wide one
            // ([#1254] review).
            footer_overflow(ui, &painter, area, rect, &tags[at..], state);
            break;
        }
        x = rect.right() + 2.0;
        let response =
            widgets::button_area(ui, rect, Id::unique(("browser-tag", epoch, tag.id)), format!("Tag {}", tag.name))
                .on_hover_text(&tag.name);
        if response.hovered() {
            // The tag's name and how many items carry it, in the status bar. The count is
            // read where the tags are kept, so it costs no copy of a list.
            let items = projects.file_library_mut().tagged_count(tag.id);
            out.status =
                Some(Status::Hint(format!("Tag {}, {} item{}", tag.name, items, if items == 1 { "" } else { "s" })));
        }
        let on = state.tag_filter.contains(&tag.id);
        if response.clicked() {
            if on {
                state.tag_filter.retain(|t| *t != tag.id);
            } else {
                state.tag_filter.push(tag.id);
            }
        }
        // Right-click: rename (typed in, with Enter or the button), recolour or remove the
        // tag. Clicks inside do not close it: a tag's new name is typed here (egui's default
        // closes a menu on a click inside it). The drafts key on the tag's id; when the menu
        // closes they go with it, and a refused rename resets the field, so a dead draft
        // does not run again by itself ([#1254] › Tag model; [#1196]).
        let was_open = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(Id::unique(("browser-footer-menu-open", epoch, tag.id))))
            .unwrap_or(false);
        // A click on the dot while its own menu stood: the menu closes for it, but the click
        // reaches the widget this frame — the typed rename commits now, before the close
        // can drop its draft ([#1254] review).
        let dot_click = ui.input(|i| {
            i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| rect.intersect(area).contains(p))
        });
        if was_open && dot_click {
            let commit_id = Id::unique(("browser-footer-rename-tag", epoch, tag.id));
            let draft: Option<String> = ui.ctx().data(|d| d.get_temp(commit_id));
            if let Some(name) = draft
                && name.trim() != tag.name
            {
                if run(projects, "library.setTag", json!({ "id": format!("#{0}", tag.id), "rename": name.trim() }), out)
                {
                    state.relist();
                    ui.ctx().data_mut(|d| d.remove::<String>(commit_id));
                } else {
                    ui.ctx().data_mut(|d| d.insert_temp(commit_id, tag.name.clone()));
                }
            }
        }
        let popup = egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let id = Id::unique(("browser-footer-rename-tag", epoch, tag.id));
                let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| tag.name.clone());
                let field = crate::kit::widgets::text_input(
                    ui,
                    egui::TextEdit::singleline(&mut name).id(id).desired_width(120.0),
                );
                field.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("Rename tag {}", tag.name))
                });
                ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
                // Enter commits, and so does a click outside the menu. A change that is
                // refused keeps the menu (the status bar says why) and the field resets.
                let committed = (field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)))
                    || ui.input(|i| {
                        i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !ui.max_rect().contains(p))
                    });
                if committed && !name.trim().is_empty() && name.trim() != tag.name {
                    if run(
                        projects,
                        "library.setTag",
                        json!({ "id": format!("#{0}", tag.id), "rename": name.trim() }),
                        out,
                    ) {
                        // The ids carry the name: the place and the filter need nothing
                        // rewritten ([#1254] › Tag model).
                        ui.ctx().data_mut(|d| d.remove::<String>(id));
                        state.relist();
                    } else {
                        ui.ctx().data_mut(|d| d.insert_temp(id, tag.name.clone()));
                    }
                }
                // The palette track colours take, then any colour.
                let current = theme::parse_hex(&tag.color).map(crate::interaction::grid::to_rgb);
                if let Some(picked) = crate::interaction::colour_picker::select_colour_row(
                    ui,
                    Id::unique(("browser-footer-tag-colour", epoch, tag.id)),
                    current,
                    "Select colour",
                ) {
                    let hex = theme::hex(crate::interaction::colour_picker::color32(picked));
                    run(projects, "library.setTag", json!({ "id": format!("#{0}", tag.id), "color": hex }), out);
                }
                ui.separator();
                let new_id = Id::unique(("browser-footer-new-tag", epoch, tag.id));
                let mut new_name: String = ui.ctx().data(|d| d.get_temp(new_id)).unwrap_or_default();
                let field = crate::kit::widgets::text_input(
                    ui,
                    egui::TextEdit::singleline(&mut new_name).id(new_id).hint_text("New tag…").desired_width(120.0),
                );
                field
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "New tag name".to_string()));
                ui.ctx().data_mut(|d| d.insert_temp(new_id, new_name.clone()));
                let with_enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let with_button = ui.button("Add tag").clicked();
                if (with_enter || with_button) && !new_name.trim().is_empty() {
                    let taken = projects.file_library_mut().tag_flat();
                    if taken.iter().any(|t| t.name == new_name.trim()) {
                        out.status = Some(Status::Error(format!("there is a tag {:?} already", new_name.trim())));
                    } else {
                        let color = tag.color.clone();
                        if run(projects, "library.setTag", json!({ "name": new_name.trim(), "color": color }), out) {
                            // The add shows itself: the footer stays where the tags section
                            // is not, and a closed one opens to show the tag ([#1254] review).
                            let closed = projects
                                .file_library_mut()
                                .sections()
                                .iter()
                                .any(|s| s.kind == SectionKind::Tags && !s.open);
                            if closed {
                                open_tags_section(projects, out);
                            }
                            reveal_added_tag(state, projects, new_name.trim());
                            out.status = Some(Status::Done(format!("Added tag {}", new_name.trim())));
                            ui.ctx().data_mut(|d| d.remove::<String>(new_id));
                            state.relist();
                            ui.close();
                        }
                    }
                }

                // The asking is the section's row menu's: the first click on Remove says what
                // will go and how many items carry it, and only the next removes ([#1250]
                // review: the two menus had drifted apart).
                let armed = state.tag_remove_confirm.as_deref() == Some(tag.name.as_str());
                let count = state.tag_counts(projects.file_library_mut()).get(&tag.id).copied().unwrap_or(0);
                let label = if armed {
                    format!("Remove {}? ({count} items)", tag.name)
                } else {
                    format!("Remove tag {}", tag.name)
                };
                if ui.button(label).clicked() {
                    if !armed && count > 0 {
                        state.tag_remove_confirm = Some(tag.name.clone());
                    } else {
                        run(projects, "library.removeTag", json!({ "id": format!("#{0}", tag.id) }), out);
                        // The removal's purge is the reconcile's: the pick among them, the
                        // place and the filter too — a save that failed leaves the memory
                        // changed all the same ([#1254] review).
                        state.reconcile(projects);
                        state.tag_remove_confirm = None;
                        state.relist();
                        ui.close();
                    }
                }
                if armed && ui.button("Keep").clicked() {
                    state.tag_remove_confirm = None;
                }
            });
        // Closed: the drafts go with it ([#1196] › keep the name with its tag). A rename
        // the field held commits first when the frame had a click: a click outside that
        // opened another menu closed this one before its body could run, and the click
        // commits as it would have ([#1254] review). The dot's own click committed its
        // way already (the draft is gone or reset), and a close without a click — Escape,
        // an add — is no commit.
        let open = popup.is_some();
        let open_key = Id::unique(("browser-footer-menu-open", epoch, tag.id));
        if was_open && !open && ui.input(|i| i.pointer.any_click()) {
            let commit_id = Id::unique(("browser-footer-rename-tag", epoch, tag.id));
            let draft: Option<String> = ui.ctx().data(|d| d.get_temp(commit_id));
            if let Some(name) = draft.as_deref().map(str::trim).filter(|n| !n.is_empty() && *n != tag.name) {
                if run(projects, "library.setTag", json!({ "id": format!("#{0}", tag.id), "rename": name }), out) {
                    ui.ctx().data_mut(|d| d.remove::<String>(commit_id));
                    state.relist();
                } else {
                    ui.ctx().data_mut(|d| d.insert_temp(commit_id, tag.name.clone()));
                }
            }
        }
        if was_open && !open {
            ui.ctx().data_mut(|d| {
                d.remove::<String>(Id::unique(("browser-footer-rename-tag", epoch, tag.id)));
                d.remove::<String>(Id::unique(("browser-footer-new-tag", epoch, tag.id)));
                crate::interaction::colour_picker::forget_state(
                    d,
                    Id::unique(("browser-footer-tag-colour", epoch, tag.id)),
                );
            });
            state.tag_remove_confirm = None;
        }
        ui.ctx().data_mut(|d| d.insert_temp(open_key, open));

        painter.circle_filled(rect.center(), DOT / 2.0, tag_color(&tag.color));
        if on {
            painter.circle_stroke(rect.center(), DOT / 2.0 + 1.5, Stroke::new(1.0, theme::browser_text_primary()));
        }
    }
    let count = match state.selected.len() {
        0 | 1 => format!("{} items", state.rows().iter().filter(|r| r.item.kind != ItemKind::Heading).count()),
        n => format!("{n} selected"),
    };
    widgets::text_in(
        &painter,
        area.shrink2(Vec2::new(PAD, 0.0)),
        Align2::RIGHT_CENTER,
        &count,
        theme::mono_font(theme::TEXT_TINY),
        theme::browser_text_hint(),
    );
}

/// The tags the footer is too narrow to draw, from one control where the next dot would
/// have gone: its menu holds every filter the footer hides, and the ring shows while one
/// of them is on ([#1254] review: a hidden one could otherwise be toggled only by moving
/// tags, which riffs back into their order).
/// The overflow menu's rows past this scroll inside their own room, no window's height for
/// a library wide in tags.
const OVERFLOW_ROWS: f32 = 10.0;
/// How tall an overflow menu's row shows.
const OVERFLOW_ROW: f32 = 22.0;

fn footer_overflow(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    rect: Rect,
    tags: &[suhina_library::Tag],
    state: &mut BrowserState,
) {
    // The tag menus' state keys on the library's epoch too: a replaced library's tag of the
    // same id opens none of the old one's menus or drafts ([#1254] review).
    let epoch = state.tag_epoch;
    let clipped = rect.intersect(area);
    // A dot the footer has no room for drops its own menu's drafts: its menu is not drawn,
    // so it cannot close by itself and clear them, and a dot coming back opens clean. The
    // footer's menus keep their state under keys of their own, never the sidebar rows'
    // ([#1254] review: shared keys cleared an open sidebar menu's draft and picker every
    // frame its tag spent in the overflow).
    ui.ctx().data_mut(|d| {
        for tag in tags {
            d.remove::<String>(Id::unique(("browser-footer-rename-tag", epoch, tag.id)));
            d.remove::<bool>(Id::unique(("browser-footer-menu-open", epoch, tag.id)));
            d.remove::<String>(Id::unique(("browser-footer-new-tag", epoch, tag.id)));
            crate::interaction::colour_picker::forget_state(
                d,
                Id::unique(("browser-footer-tag-colour", epoch, tag.id)),
            );
        }
    });
    let response = widgets::button_area(ui, clipped, Id::unique("browser-footer-more"), "More tags");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "More tags".to_string()));
    let fill = widgets::hover(ui, &response);
    painter.rect_filled(clipped, 0.0, theme::panel_hover().gamma_multiply(fill));
    painter.text(
        clipped.center(),
        Align2::CENTER_CENTER,
        "…",
        theme::ui_font(theme::TEXT_SMALL),
        theme::browser_text_secondary(),
    );
    if tags.iter().any(|t| state.tag_filter.contains(&t.id)) {
        painter.circle_stroke(clipped.center(), DOT / 2.0 + 1.5, Stroke::new(1.0, theme::browser_text_primary()));
    }
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::TOP_START)
        .gap(2.0)
        .show(|ui| {
            // The rows toggle the filters, and the menu stays for more than one. A library
            // wide in tags hides more rows than a window holds: the rows scroll inside
            // their own room, so each one is reachable by the pointer ([#1254] review).
            egui::ScrollArea::vertical().max_height(OVERFLOW_ROWS * OVERFLOW_ROW).show(ui, |ui| {
                for tag in tags {
                    let on = state.tag_filter.contains(&tag.id);
                    let label = format!("Tag {}", tag.name);
                    let row = ui.add(egui::Button::selectable(on, &label).min_size(Vec2::new(0.0, OVERFLOW_ROW)));
                    // The folded state is its own: what the label says stays, so tests and
                    // readers find the filter they mean, and the screen lets the ring's
                    // state read ([#1254] review).
                    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label.clone()));
                    if let Some(color) = theme::parse_hex(&tag.color) {
                        let at = row.rect.left_center() + Vec2::new(8.0, 0.0);
                        ui.painter().circle_filled(at, 3.0, color);
                        if on {
                            ui.painter().circle_stroke(at, 4.5, Stroke::new(1.0, theme::browser_text_primary()));
                        }
                    }
                    if row.clicked() {
                        if on {
                            state.tag_filter.retain(|t| *t != tag.id);
                        } else {
                            state.tag_filter.push(tag.id);
                        }
                    }
                }
            });
        });
}

/// The browser's resize handle at its inner edge: dragged, the width it is dragged to;
/// double-clicked, `default`. Gives that width, and whether the handle was let go (or reset)
/// now: the width is saved then, not on every frame of a drag.
pub fn resize_handle(ui: &Ui, handle: Rect, width: f32, left: bool, default: f32) -> (Option<f32>, bool) {
    let response = ui
        .interact(handle, Id::unique("browser-resize"), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Browser splitter"));
    if response.double_clicked() {
        return (Some(default), true);
    }
    {
        // `#browser-splitter` (a `divider`, with `:hover` and `:active`) takes the element's
        // `background-color`; without one, the border colour, stronger under the pointer.
        let style = DIVIDER.id("browser-splitter").response(&response).enter();
        let native = if response.hovered() || response.dragged() { theme::border_strong() } else { theme::border() };
        ui.painter().rect_filled(handle, 0.0, style.background_or(native));
    }
    // The width follows the pointer from where the drag began, so it never falls behind it
    // (adding each frame's movement to the width rounded to whole points would).
    let start_id = Id::unique("browser-resize-start");
    if response.drag_started()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        ui.ctx().data_mut(|d| d.insert_temp(start_id, (width, pointer.x)));
    }
    if response.dragged()
        && let (Some((from, x0)), Some(pointer)) =
            (ui.ctx().data(|d| d.get_temp::<(f32, f32)>(start_id)), response.interact_pointer_pos())
    {
        let dx = pointer.x - x0;
        return (Some(if left { from + dx } else { from - dx }), false);
    }
    (None, response.drag_stopped())
}

/// The active project's session.
fn active(projects: &Projects) -> Option<&suhina_session::Session> {
    projects.active().and_then(|id| projects.get(id))
}

/// A place or kind in the browser's left column, `browser-place`: `:hover`, `:active`, and
/// `[checked]` and `[selected]` while it is the one shown.
fn place_style(response: &egui::Response, chosen: bool) -> Entered {
    BROWSER_PLACE.response(response).attr(Checked, chosen).attr(Selected, chosen).enter()
}

/// A row's background: the sheet's `background-color` for `el`, the row's element, else `fill`,
/// and its gradients and shadows.
fn paint_row(painter: &egui::Painter, el: &Entered, rect: Rect, fill: egui::Color32) {
    painter.rect_filled(rect, 0.0, el.background_or(fill));
    el.paint_background(painter, rect, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name of a plug-in always shows: as the panel narrows, the columns to its right drop
    /// (the rightmost first) before the name gets narrower than its least.
    #[test]
    fn the_name_column_keeps_its_room_and_the_others_drop_from_the_right() {
        let wide = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 20.0));
        assert_eq!(columns_shown(wide), 1);
        let (name_left, name_right) = column_span(wide, 0).unwrap();
        let (format_left, format_right) = column_span(wide, 1).unwrap();
        assert!(name_right <= format_left && format_right <= wide.right() && name_left >= wide.left());
        assert!(name_right - name_left >= NAME_LEAST);
        let narrow = Rect::from_min_size(Pos2::ZERO, Vec2::new(NAME_LEAST + 30.0, 20.0));
        assert_eq!(columns_shown(narrow), 0);
        assert!(column_span(narrow, 1).is_none());
        let (left, right) = column_span(narrow, 0).unwrap();
        assert_eq!(right - left, narrow.width() - 2.0 * PAD, "the name takes the whole row");
    }

    /// The Tags section's top-level folder sits a step in from the section's title: its chevron
    /// is not under the title's own, and each level goes a step further ([#1254] review: Tags
    /// and its first folder read as one level).
    #[test]
    fn the_tags_rows_sit_a_step_in_from_their_title() {
        let left = 0.0;
        let top_folder_chevron = tag_row_lead(left, 0) + 5.0;
        assert!(top_folder_chevron - title_chevron_x(left) >= 6.0, "{top_folder_chevron}");
        assert_eq!(tag_row_lead(left, 1) - tag_row_lead(left, 0), INDENT, "a level is a step");
        // The deepest level still leaves the name room inside the sidebar.
        let deepest = tag_row_lead(left, suhina_library::store::MAX_TAG_DEPTH);
        assert!(SIDEBAR - deepest >= 30.0, "{deepest}");
    }

    /// Two dots of a row tag the same item, so their centers are close: their hit areas must
    /// touch at most, or a pointer inside one would fall in the other too and answer with the
    /// wrong tag (egui gives the last-added of two overlapping widgets the input).
    #[test]
    fn adjacent_tag_dots_have_hit_areas_that_do_not_overlap() {
        let (first, second) = (tag_dot_center(100.0, 0, 40.0), tag_dot_center(100.0, 1, 40.0));
        let (a, b) =
            (Rect::from_center_size(first, Vec2::splat(DOT_HIT)), Rect::from_center_size(second, Vec2::splat(DOT_HIT)));
        assert_eq!(b.left(), a.right(), "neighbouring areas touch: {a:?} and {b:?}");
        // Nothing inside a dot's area falls in its neighbour's: a point just right of the
        // first dot's edge is the first's only, so egui's hit test cannot pick the wrong one.
        for point in [first, Pos2::new(a.right() - 0.5, first.y), Pos2::new(a.left() + 0.5, first.y)] {
            assert!(a.contains(point) && !b.contains(point), "{point:?} is in both hit areas");
        }
        // The area still holds the dot drawn inside it, and is larger than the dot.
        assert!(a.width() > DOT && a.height() > DOT);
    }
}
