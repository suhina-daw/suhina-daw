// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Draws the browser panel (`docs/spec/browser.md` › Layout; sizes from
//! `docs/design/components.md` › Browser): the search, the category filters, the sidebar of
//! places, the results, the preview strip and the footer. Library changes and previews run
//! their commands (`library.*`, `preview.*`); adding to the project is left to the window,
//! which knows the selected track.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Align2, Color32, CursorIcon, Id, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_library::{FolderState, Group, Item};
use suhina_session::{Category, Projects};

use crate::browser::{BrowserState, Place, Row, duration_text};
use crate::interaction::drop::{BrowserItem, ItemKind};
use crate::interaction::status::Status;
use crate::kit::theme::{self, BROWSER_ROW as ROW, PAD};
use crate::kit::widgets::play_glyph;
use crate::kit::{motion, widgets};
use crate::settings::Settings;

pub const SEARCH: f32 = 26.0;
pub const FILTER: f32 = 16.0;
pub const SIDEBAR: f32 = 104.0;
pub const INDENT: f32 = 12.0;

/// Clicks left of this on a row that opens are its chevron: the triangle sits 10 pt wide,
/// 4 pt in from the row's indent.
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
/// A row of the tag editor.
const EDITOR_ROW: f32 = 26.0;

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
        ItemKind::Folder | ItemKind::Heading | ItemKind::Project => theme::text_secondary(),
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
    theme::parse_hex(text).unwrap_or_else(theme::text_hint)
}

/// Runs a library or preview command for the browser; an error goes to the status bar.
fn run(projects: &mut Projects, id: &str, params: serde_json::Value, out: &mut BrowserOut) -> bool {
    match projects.execute(id, &params) {
        Ok(_) => true,
        Err(e) => {
            out.status = Some(Status::Error(e.message().to_owned()));
            false
        }
    }
}

/// Draws the browser in `area`.
pub fn show(ui: &mut Ui, area: Rect, state: &mut BrowserState, cx: Context<'_>) -> BrowserOut {
    let mut out = BrowserOut::default();
    let Context { projects, settings, selected_track, search_key, other_results } = cx;
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::panel_background());
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
    // Samples is.
    let mut y = search.bottom() + 4.0;
    let mut x = area.left() + PAD;
    let small = theme::ui_font(theme::TEXT_SMALL);
    let map = other_results.is_some();
    let recent = matches!(state.place, Some(Place::Recent(_)));
    let samples = state.place.is_none() && state.category == Category::Samples;
    let chips: Vec<(String, Chip)> = if recent && settings.view.recent_chips {
        std::iter::once(("All".to_string(), Chip::Recent(None)))
            .chain(Group::ALL.into_iter().map(|g| (g.title().to_string(), Chip::Recent(Some(g)))))
            .collect()
    } else if samples && map {
        vec![("List".to_string(), Chip::List), (SAMPLE_MAP.to_string(), Chip::Map)]
    } else {
        Vec::new()
    };
    let showing_map = state.mode == crate::browser::ResultsMode::Other;
    for (title, chip) in &chips {
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
        let response = widgets::button_area(ui, rect, Id::new(("browser-filter", title.as_str())), label);
        let chosen = match chip {
            Chip::Recent(group) => state.place == Some(Place::Recent(*group)),
            Chip::List => !showing_map,
            Chip::Map => showing_map,
        };
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
        painter.rect_filled(rect, 0.0, motion::mix(idle, theme::text_value(), lit));
        let text = motion::mix(theme::text_secondary(), theme::window_background(), lit);
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, title, small.clone(), text);
        x = rect.right() + FILTER_GAP;
    }
    // The map belongs to the samples: anywhere else the list shows.
    if !samples && !recent {
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
    let edit_rect = footer_view(ui, footer, state, projects, &mut out);
    // The tag editor's popup right above the Edit tags button, drawn last so the strip and
    // the footer it floats over do not paint over it. A click outside the popup, on the
    // list or anywhere else in the window, closes it.
    let editor_popup = state.tag_editor.then(|| {
        let rows = projects.file_library_mut().tags().len() + 1; // + the row that adds one
        let width = 260.0_f32.min((results.width() - 8.0).max(150.0));
        let height = (rows as f32 * (EDITOR_ROW + 2.0) + PAD).min(results.height() - 4.0);
        Rect::from_min_max(
            Pos2::new(edit_rect.right() - width, footer.top() - height),
            Pos2::new(edit_rect.right(), footer.top()),
        )
    });
    if let Some(popup) = editor_popup {
        tag_editor(ui, popup, state, projects, &mut out);
        if ui.input(|i| i.pointer.any_click())
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !popup.contains(p) && !edit_rect.contains(p))
        {
            state.tag_editor = false;
            state.tag_editor_confirm = None;
        }
    }

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
            Ok(_) => state.previewing = Some(path.display().to_string()),
            Err(e) => out.status = Some(Status::Error(format!("Cannot preview: {}", e.message().to_owned()))),
        }
    }
    if let Some(e) = state.take_preview_error() {
        out.status = Some(Status::Error(format!("Cannot preview: {e}")));
    }
    // While a sample is being read for its preview, every frame looks for it, so it plays the
    // moment it is ready.
    if state.preview_pending() {
        ui.ctx().request_repaint();
    } else if state.previewing.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    }
    out
}

fn search_field(ui: &mut Ui, rect: Rect, state: &mut BrowserState, key: &str, out: &mut BrowserOut) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::window_background());
    let glass = Pos2::new(rect.left() + PAD + 5.0, rect.center().y);
    painter.circle_stroke(glass + Vec2::new(-1.0, -1.0), 3.5, Stroke::new(1.1, theme::text_hint()));
    painter
        .line_segment([glass + Vec2::new(1.5, 1.5), glass + Vec2::new(4.5, 4.5)], Stroke::new(1.1, theme::text_hint()));
    let hint = theme::mono_font(theme::TEXT_TINY);
    let key_width = widgets::text_width(ui, key, hint.clone());
    let field = Rect::from_min_max(
        Pos2::new(rect.left() + PAD + 14.0, rect.top() + 4.0),
        Pos2::new(rect.right() - PAD - key_width - 6.0, rect.bottom() - 4.0),
    );
    let id = Id::new("browser-search");
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
        .hint_text(egui::RichText::new("Search").color(theme::text_hint()))
        .font(theme::ui_font(theme::TEXT))
        .text_color(theme::text_primary());
    let response = ui.put(field, edit);
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
        theme::text_hint(),
    );
}

/// Moves the selection `step` rows (from the search field's arrow keys).
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
    let mut y = area.top();
    let title = |y: &mut f32, text: &str| {
        *y += 8.0;
        let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, *y), Vec2::new(area.width() - PAD, 12.0));
        widgets::text_in(&painter, rect, Align2::LEFT_CENTER, text, title_font.clone(), theme::text_hint());
        *y += 12.0;
    };
    let mut entry = |ui: &mut Ui, y: &mut f32, target: Target, label: &str, note: Option<&str>| -> egui::Response {
        let rect = Rect::from_min_size(Pos2::new(area.left(), *y), Vec2::new(area.width(), ROW));
        *y += ROW;
        let key = match &target {
            Target::Place(place) => place.param(),
            Target::Category(category) => format!("library:{}", category.title()),
        };
        let response = ui.interact(rect.intersect(area), Id::new(("browser-place", key.as_str())), Sense::click());
        // Screen readers and tests: "Show Samples" for the library's kinds, "Place Recent MIDI"
        // for a recent kind, "Place Pinned" and the like for the rest.
        let name = match &target {
            Target::Category(category) => format!("Show {}", category.title()),
            Target::Place(Place::Recent(Some(group))) => format!("Place Recent {}", group.title()),
            Target::Place(_) => format!("Place {}", label.trim()),
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
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
                    let fill = if chosen {
                        theme::control_active()
                    } else {
                        motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response))
                    };
                    painter.rect_filled(rect.intersect(area), 0.0, fill);
                    let text = Rect::from_min_max(rect.min + Vec2::new(PAD + 2.0, 0.0), rect.max - Vec2::new(PAD, 0.0));
                    let color = if chosen { theme::text_primary() } else { theme::text_secondary() };
                    widgets::text_in(&painter, text, Align2::LEFT_CENTER, label, item_font.clone(), color);
                }
                return response;
            }
        };
        let fill = if chosen {
            theme::control_active()
        } else {
            motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response))
        };
        if rect.intersects(area) {
            painter.rect_filled(rect.intersect(area), 0.0, fill);
            let text = Rect::from_min_max(rect.min + Vec2::new(PAD + 2.0, 0.0), rect.max - Vec2::new(PAD, 0.0));
            let color = if chosen { theme::text_primary() } else { theme::text_secondary() };
            widgets::text_in(&painter, text, Align2::LEFT_CENTER, label, item_font.clone(), color);
            if let Some(note) = note.filter(|_| !chosen) {
                widgets::text_in(
                    &painter,
                    text,
                    Align2::RIGHT_CENTER,
                    note,
                    theme::mono_font(theme::TEXT_TINY),
                    theme::text_hint(),
                );
            }
        }
        // The place chosen shows a × that clears it; a second click on the place keeps it.
        if chosen && rect.intersects(area) {
            let clear = Rect::from_min_size(Pos2::new(rect.right() - ROW, rect.top()), Vec2::splat(ROW));
            let cleared = widgets::button_area(
                ui,
                clear.intersect(area),
                Id::new(("browser-place-clear", place.param())),
                "Show everything",
            );
            let color = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &cleared));
            widgets::text_in(&painter, clear, Align2::CENTER_CENTER, "×", item_font.clone(), color);
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
    };
    title(&mut y, "Pinned");
    entry(ui, &mut y, Target::Place(Place::Pinned), "Pinned", None);
    entry(ui, &mut y, Target::Place(Place::Recent(None)), "Recent", None);
    if !recent_chips {
        for group in Group::ALL {
            entry(ui, &mut y, Target::Place(Place::Recent(Some(group))), &format!("    {}", group.title()), None);
        }
    }
    title(&mut y, "Library");
    for category in Category::ALL {
        entry(ui, &mut y, Target::Category(category), category.title(), None);
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
        let response = entry(ui, &mut y, Target::Place(Place::Folder(folder.path.clone())), &name, note);
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
    match state.new_folder.as_mut() {
        None => {
            let response = widgets::button_area(ui, rect.intersect(area), Id::new("browser-add-folder"), "Add folder");
            let color = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &response));
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
            let id = Id::new("browser-new-folder");
            let edit = egui::TextEdit::singleline(text)
                .id(id)
                .hint_text("A folder's path")
                .font(item_font.clone())
                .text_color(theme::text_primary());
            let field = rect.shrink2(Vec2::new(4.0, 1.0));
            let browse =
                Rect::from_min_size(Pos2::new(field.right() - 56.0, field.top()), Vec2::new(56.0, field.height()));
            if crate::kit::widgets::button(ui, &painter, browse.intersect(area), "Browse…", "Browse for a folder") {
                out.pick_folder = true;
                state.new_folder = None;
                return;
            }
            let response = ui.put(
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
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::panel_background());
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
    if rows.is_empty() {
        let text = if state.search.trim().is_empty() && projects.file_library_mut().folders().is_empty() {
            "Add a folder to find samples"
        } else {
            "Nothing found"
        };
        widgets::text_in(
            &painter,
            area.shrink(PAD),
            Align2::CENTER_TOP,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
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
    let visible = (area.height() / ROW).ceil() as usize + 1;
    let tags = projects.file_library_mut().tags();
    let mono = theme::mono_font(theme::TEXT_TINY);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut clicked: Option<(usize, Modifiers)> = None;
    let mut pressed: Option<(usize, Modifiers)> = None;
    let mut double: Option<usize> = None;
    let mut drag: Option<usize> = None;
    let mut play: Option<usize> = None;
    for index in first..(first + visible).min(rows.len()) {
        let Some(row) = rows.get(index) else { continue };
        let top = area.top() + index as f32 * ROW - state.scroll;
        let rect = Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), ROW));
        let shown = rect.intersect(area);
        if shown.height() <= 0.0 {
            continue;
        }
        let response = ui.interact(shown, Id::new(("browser-row", row.node())), Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Browser {}", row.item.name))
        });
        let selected = state.selected.iter().any(|s| s.key == row.item.key);
        let fill = if selected {
            theme::control_active()
        } else {
            motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response))
        };
        painter.rect_filled(shown, 0.0, fill);
        let mut x = rect.left() + 4.0 + row.depth as f32 * INDENT;
        let lead = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(10.0, ROW));
        let previewable = matches!(row.item.kind, ItemKind::Sample | ItemKind::Midi);
        if row.opens {
            widgets::chevron(&painter, lead.center(), row.open, theme::text_hint());
        } else if previewable && (selected || response.hovered()) {
            let button = widgets::button_area(
                ui,
                lead,
                Id::new(("browser-play", row.node())),
                format!("Preview {}", row.item.name),
            );
            let playing = state.previewing.as_deref() == Some(row.item.key.as_str());
            play_glyph(
                &painter,
                lead.center(),
                playing,
                if button.hovered() { theme::text_primary() } else { theme::text_secondary() },
                7.0,
            );
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
                widgets::text_in(&painter, span, Align2::RIGHT_CENTER, columns.format, small, theme::text_hint());
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
        let color = if selected { theme::text_primary() } else { theme::text_value() };
        let galley = widgets::fitted(&painter, &row.item.name, font.clone(), color, (name_space - dots).max(0.0));
        let name_pos = Pos2::new(x, rect.center().y - galley.size().y / 2.0);
        let name_end = name_pos.x + galley.size().x;
        painter.galley(name_pos, galley, color);
        for (i, name) in item_tags.iter().enumerate() {
            if let Some(tag) = tags.iter().find(|t| t.name == *name) {
                let center = tag_dot_center(name_end, i, rect.center().y);
                painter.circle_filled(center, DOT / 2.0, tag_color(&tag.color));
                // The dot answers a hover with its tag's name: a tooltip, the status bar, and
                // an accessible label.
                let dot = ui
                    .interact(
                        Rect::from_center_size(center, Vec2::splat(DOT_HIT)),
                        Id::new(("browser-tag-dot", row.node(), name.as_str())),
                        Sense::hover(),
                    )
                    .on_hover_text(format!("Tag {name}"));
                dot.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Tagged {name}")));
                if dot.hovered() {
                    out.status = Some(Status::Hint(format!("Tag {name}")));
                }
            }
        }
        if !meta.is_empty() {
            let meta_rect =
                Rect::from_min_max(Pos2::new(rect.right() - meta_width, rect.top()), rect.max - Vec2::new(PAD, 0.0));
            widgets::text_in(&painter, meta_rect, Align2::RIGHT_CENTER, &meta, mono.clone(), theme::text_hint());
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
        // is typed into (egui's default closes a menu on a click inside it too).
        egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| row_menu(ui, &item, &tags, state, projects, out));
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
        // leaves it open.
        let on_chevron = row.opens
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| p.x < chevron_zone(area.left(), row.depth));
        if row.opens && !on_chevron && !opens_on_click(row) {
            state.toggle(row.node());
        } else if row.item.kind == ItemKind::Project {
            out.open_project = row.item.path();
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
        if state.previewing.as_deref() == Some(row.item.key.as_str()) {
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
        let items: Vec<BrowserItem> =
            items.into_iter().filter(|i| i.kind != ItemKind::Heading && i.kind != ItemKind::Folder).collect();
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
    let pinned = projects.file_library_mut().pinned().contains(&item.key);
    if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
        run(projects, if pinned { "library.unpin" } else { "library.pin" }, json!({ "item": item.key }), out);
        state.relist();
        ui.close();
    }
    let carried = projects.file_library_mut().item_tags(&item.key);
    ui.menu_button("Tag", |ui| {
        for tag in tags {
            let on = carried.contains(&tag.name);
            if tag_entry(ui, tag, on).clicked() {
                run(projects, "library.tag", json!({ "item": item.key, "tag": tag.name, "on": !on }), out);
                state.relist();
                ui.close();
            }
        }
        ui.separator();
        // A new tag, set on the item at once.
        let id = Id::new("browser-new-tag");
        let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let response = ui.add(egui::TextEdit::singleline(&mut name).hint_text("New tag…").desired_width(120.0));
        ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !name.trim().is_empty() {
            let palette = suhina_library::store::default_tags();
            let color =
                palette.get(tags.len() % palette.len().max(1)).map_or("#8C8A85".to_string(), |t| t.color.clone());
            let name = name.trim().to_string();
            if run(projects, "library.setTag", json!({ "name": name, "color": color }), out) {
                run(projects, "library.tag", json!({ "item": item.key, "tag": name, "on": true }), out);
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
    if item.kind == ItemKind::Folder && ui.button("Refresh").clicked() {
        state.relist();
        ui.close();
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
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, &tag.name));
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
        painter.add(egui::Shape::line(tick.to_vec(), Stroke::new(1.5, theme::window_background())));
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
                state.previewing = Some(item.key.clone());
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
            Id::new(("browser-plugins-view", view.name())),
            format!("Show plug-in {}", name.to_lowercase()),
        );
        if response.clicked() && chosen != view {
            run(projects, "library.setPluginsView", json!({ "view": view.name() }), out);
        }
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, if chosen == view { theme::text_value() } else { idle });
        let text = if chosen == view { theme::window_background() } else { theme::text_secondary() };
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, name, small.clone(), text);
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
        let response = widgets::button_area(ui, rect, Id::new(("browser-column", index)), format!("Sort by {name}"));
        if response.clicked() {
            state.plugin_sort = (index as u8, if sorted == index as u8 { !descending } else { false });
        }
        let chosen = sorted == index as u8;
        let label =
            if chosen { format!("{name} {}", if descending { "▼" } else { "▲" }) } else { name.to_string() };
        let color = if chosen { theme::text_primary() } else { theme::text_hint() };
        let hover = motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response));
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

/// Stops any preview.
pub fn stop_preview(state: &mut BrowserState, projects: &mut Projects) {
    let was = state.previewing.is_some() || state.preview_pending();
    state.cancel_preview();
    if was {
        let _ = projects.execute("preview.stop", &json!({}));
    }
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
    painter.rect_filled(area, 0.0, theme::window_background());
    painter.hline(area.x_range(), area.top() + 0.5, theme::hairline(theme::border()));
    // The Auto switch at the top right: a selected sample plays by itself while it is lit
    // (Settings › Browser › the same setting).
    let auto = Rect::from_min_size(Pos2::new(area.right() - PAD - 40.0, area.top() + 4.0), Vec2::new(40.0, 16.0));
    let response = ui
        .interact(auto, Id::new("browser-preview-auto"), Sense::click())
        .on_hover_text("Auto preview: a sample plays when it is selected");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, true, "Auto preview"));
    if response.clicked() {
        out.toggle_autoplay = true;
    }
    let (fill, text) = widgets::toggle_colors(ui, &response, settings.preview_autoplay);
    painter.rect_filled(auto, 3.0, fill);
    widgets::text_in(&painter, auto, Align2::CENTER_CENTER, "Auto", theme::ui_font(theme::TEXT_SMALL), text);
    let area = Rect::from_min_max(area.min, Pos2::new(auto.left() - PAD, area.bottom()));
    let item = match state.selected.as_slice() {
        [one] if matches!(one.kind, ItemKind::Sample | ItemKind::Midi) => one.clone(),
        _ => {
            widgets::text_in(
                &painter,
                area.shrink(PAD),
                Align2::LEFT_CENTER,
                "Select a sample or MIDI file to preview",
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_hint(),
            );
            return;
        }
    };
    let Some(path) = item.path() else { return };
    let monitor = projects
        .preview_session()
        .and_then(|s| s.player())
        .map(|p| (p.preview_monitor().playing(), p.preview_monitor().seconds()));
    let playing_this = state.previewing.as_deref() == Some(item.key.as_str()) && monitor.is_some_and(|m| m.0);
    if state.previewing.as_deref() == Some(item.key.as_str())
        && monitor.is_some_and(|m| !m.0)
        && !state.preview_pending()
    {
        state.previewing = None;
    }
    let info = state.info(&path, true).cloned().unwrap_or_default();

    // Top line: play, name, length, tempo, key.
    let top = Rect::from_min_size(area.min + Vec2::new(PAD, 4.0), Vec2::new(area.width() - PAD * 2.0, PLAY));
    let play = Rect::from_min_size(top.min, Vec2::splat(PLAY));
    let response = widgets::button_area(ui, play, Id::new("browser-preview-play"), "Preview");
    painter.rect_filled(
        play,
        0.0,
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
    );
    play_glyph(
        &painter,
        play.center(),
        playing_this,
        if playing_this { theme::accent() } else { theme::text_primary() },
        7.0,
    );
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
    widgets::text_in(&painter, text_rect, Align2::RIGHT_CENTER, &facts, mono, theme::text_secondary());
    let name_rect =
        Rect::from_min_max(text_rect.min, Pos2::new(text_rect.right() - facts_width - PAD, text_rect.bottom()));
    widgets::text_in(
        &painter,
        name_rect,
        Align2::LEFT_CENTER,
        &item.name,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
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
        crate::kit::waveform::paint(
            &clipped(done),
            &shape,
            crate::kit::waveform::Colors::from(theme::text_secondary()),
        );
        crate::kit::waveform::paint(&clipped(rest), &shape, crate::kit::waveform::Colors::from(theme::text_hint()));
    } else if let Some(notes) = &info.notes {
        let (low, high) = notes.iter().fold((127u8, 0u8), |(l, h), n| (l.min(n.2), h.max(n.2)));
        let span = f32::from(high.saturating_sub(low).max(1));
        let beats = info.beats.max(1.0);
        for (start, length, key) in notes.iter() {
            let x0 = wave.left() + (*start / beats) as f32 * wave.width();
            let x1 = wave.left() + ((start + length) / beats) as f32 * wave.width();
            let y = wave.bottom() - f32::from(key - low) / span * (wave.height() - 2.0) - 2.0;
            let color = if x0 <= played_x { theme::text_secondary() } else { theme::text_hint() };
            painter.rect_filled(Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.0), y + 2.0)), 0.0, color);
        }
    } else {
        widgets::text_in(
            &painter,
            wave,
            Align2::LEFT_CENTER,
            "Reading…",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    if playing_this {
        painter.vline(played_x, wave.y_range(), Stroke::new(1.0, theme::accent()));
    }
    // A click on the waveform previews from there.
    let response =
        ui.interact(wave, Id::new("browser-preview-wave"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
    if response.clicked()
        && total > 0.0
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let from = f64::from((pointer.x - wave.left()) / wave.width().max(1.0)) * total;
        start_preview(state, projects, settings, track, &item, from.max(0.0), out);
    }
}

/// The browser's tag editor, the popup right above the footer's Edit tags button: every tag
/// in a list — its colour, its name (typed to rename), how many items carry it and Remove,
/// with a confirm; the last row adds one. Its checkbox filters by the tag, as the footer's
/// dots do, or with items selected applies the tag to them, as a row's Tag menu does
/// ([browser](browser.md) › Colour tags).
fn tag_editor(ui: &mut Ui, area: Rect, state: &mut BrowserState, projects: &mut Projects, out: &mut BrowserOut) {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    painter.rect_stroke(area, 1.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    // How many items carry each tag, counted when the editor opened and after every change
    // made in it.
    if state.tag_counts.is_none() {
        let mut counts = HashMap::new();
        for (_, names) in projects.file_library_mut().tagged() {
            for name in names {
                *counts.entry(name).or_default() += 1;
            }
        }
        state.tag_counts = Some(counts);
    }
    let counts = state.tag_counts.clone().unwrap_or_default();
    let tags = projects.file_library_mut().tags();
    let mut y = area.top() + 2.0;
    for tag in &tags {
        let rect = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), EDITOR_ROW));
        if rect.bottom() > area.bottom() - 2.0 {
            break; // more tags than the popup's room; the footer's dots hold the rest
        }
        y = rect.bottom() + 2.0;
        let row = ui.interact(rect, Id::new(("tag-editor-row", &tag.name)), Sense::hover());
        row.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Tag row {} ({} items)", tag.name, counts.get(&tag.name).copied().unwrap_or(0)),
            )
        });
        let on = state.tag_filter.contains(&tag.name);
        let fill = if on {
            theme::control_active()
        } else {
            motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &row))
        };
        painter.rect_filled(rect, 0.0, fill);
        // Its checkbox: with items selected, it tags them (ticked while every selected item
        // carries the tag); with none, it filters the results by the tag, as the footer's
        // dots do.
        let selection: Vec<String> = state
            .selected
            .iter()
            .filter(|s| s.kind != ItemKind::Heading && s.kind != ItemKind::Folder)
            .map(|s| s.key.clone())
            .collect();
        let on = if selection.is_empty() {
            state.tag_filter.contains(&tag.name)
        } else {
            selection.iter().all(|key| projects.file_library_mut().item_tags(key).contains(&tag.name))
        };
        let box_rect = Rect::from_min_size(Pos2::new(rect.left() + 4.0, rect.center().y - 7.0), Vec2::splat(14.0));
        let check = ui.interact(box_rect, Id::new(("tag-editor-filter", &tag.name)), Sense::click());
        check.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Checkbox,
                true,
                if selection.is_empty() {
                    format!("Filter tag {}", tag.name)
                } else {
                    format!("Apply tag {}", tag.name)
                },
            )
        });
        painter.rect_filled(box_rect, 3.0, if on { theme::text_value() } else { theme::control_background() });
        if on {
            painter.rect_filled(box_rect.shrink(4.0), 0.0, theme::window_background());
        }
        if check.clicked() {
            if selection.is_empty() {
                if on {
                    state.tag_filter.retain(|t| *t != tag.name);
                } else {
                    state.tag_filter.push(tag.name.clone());
                }
            } else {
                for key in &selection {
                    if let Err(e) = projects.file_library_mut().tag(key, &tag.name, !on) {
                        out.status = Some(Status::Error(e));
                    }
                }
                state.tag_counts = None;
                state.relist();
            }
        }
        // Its colour.
        let mut color = tag_color(&tag.color);
        let pick_rect =
            Rect::from_min_size(Pos2::new(box_rect.right() + 4.0, rect.center().y - FILTER / 2.0), Vec2::splat(FILTER));
        let picked = ui
            .scope_builder(egui::UiBuilder::new().max_rect(pick_rect).id_salt(("tag-editor-color", &tag.name)), |ui| {
                egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::Opaque)
            })
            .inner;
        if picked.changed() {
            let hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
            run(projects, "library.setTag", json!({ "name": tag.name, "color": hex }), out);
            state.tag_counts = None;
        }
        // Its name, typed to rename: the field takes the room between the colour and the
        // fixed right side, at least enough to type in.
        let armed = state.tag_editor_confirm.as_deref() == Some(tag.name.as_str());
        let remove_width = 58.0;
        let keep_width = if armed { 44.0 } else { 0.0 };
        let count_width = if area.width() >= 240.0 { 46.0 } else { 0.0 };
        let field_left = pick_rect.right() + 4.0;
        let field_right = (rect.right() - remove_width - keep_width - count_width - 10.0).max(field_left + 48.0);
        let field_rect = Rect::from_min_max(Pos2::new(field_left, rect.top()), Pos2::new(field_right, rect.bottom()));
        let id = Id::new(("tag-editor-name", &tag.name));
        let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| tag.name.clone());
        let field = ui.put(
            field_rect,
            egui::TextEdit::singleline(&mut name)
                .id(id)
                .desired_width(field_rect.width())
                .font(theme::ui_font(theme::TEXT_SMALL)),
        );
        ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
        field.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Tag name {}", tag.name))
        });
        if field.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && !name.trim().is_empty()
            && name.trim() != tag.name
            && run(projects, "library.setTag", json!({ "name": tag.name, "rename": name.trim() }), out)
        {
            for filter in state.tag_filter.iter_mut().filter(|f| **f == tag.name) {
                *filter = name.trim().to_string();
            }
            if state.tag_editor_confirm.as_deref() == Some(tag.name.as_str()) {
                state.tag_editor_confirm = None;
            }
            state.tag_counts = None;
            state.relist();
            ui.ctx().data_mut(|d| d.remove::<String>(id));
        }
        // How many items carry it.
        if count_width > 0.0 {
            let count_rect = Rect::from_min_max(
                Pos2::new(field_rect.right() + 5.0, rect.top()),
                Pos2::new(field_rect.right() + 5.0 + count_width, rect.bottom()),
            );
            widgets::text_in(
                &painter,
                count_rect,
                Align2::RIGHT_CENTER,
                &format!("{} items", counts.get(&tag.name).copied().unwrap_or(0)),
                theme::mono_font(theme::TEXT_TINY),
                theme::text_hint(),
            );
        }
        // Remove, armed by the first click and done by the second; Keep stands it down.
        let remove_rect = Rect::from_min_size(
            Pos2::new(field_rect.right() + 5.0 + count_width, rect.center().y - FILTER / 2.0),
            Vec2::new(remove_width, FILTER),
        );
        let remove_label = if armed { format!("Remove {}?", tag.name) } else { "Remove".to_string() };
        let remove = widgets::button_area(
            ui,
            remove_rect,
            Id::new(("tag-editor-remove", &tag.name)),
            format!("Remove tag {}", tag.name),
        );
        widgets::text_in(
            &painter,
            remove_rect,
            Align2::CENTER_CENTER,
            &remove_label,
            theme::ui_font(theme::TEXT_SMALL),
            if armed { theme::recording() } else { theme::text_secondary() },
        );
        if remove.clicked() {
            if armed {
                if run(projects, "library.removeTag", json!({ "name": tag.name }), out) {
                    state.tag_filter.retain(|t| *t != tag.name);
                }
                state.tag_editor_confirm = None;
                state.tag_counts = None;
                state.relist();
            } else {
                state.tag_editor_confirm = Some(tag.name.clone());
            }
        }
        if armed {
            let keep_rect = Rect::from_min_size(
                Pos2::new(remove_rect.right() + 4.0, rect.center().y - FILTER / 2.0),
                Vec2::new(keep_width, FILTER),
            );
            let keep = widgets::button_area(ui, keep_rect, Id::new(("tag-editor-keep", &tag.name)), "Keep");
            widgets::text_in(
                &painter,
                keep_rect,
                Align2::CENTER_CENTER,
                "Keep",
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_secondary(),
            );
            if keep.clicked() {
                state.tag_editor_confirm = None;
            }
        }
    }
    // The last row adds a tag: a name and the next default colour, the field the popup wide.
    if y + EDITOR_ROW <= area.bottom() - 2.0 {
        let rect = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), EDITOR_ROW));
        let id = Id::new("tag-editor-new");
        let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
        let field_rect =
            Rect::from_min_max(Pos2::new(area.left() + PAD, rect.top()), Pos2::new(area.right() - PAD, rect.bottom()));
        let field = ui.put(
            field_rect,
            egui::TextEdit::singleline(&mut name)
                .id(id)
                .hint_text("New tag…")
                .desired_width(field_rect.width())
                .font(theme::ui_font(theme::TEXT_SMALL)),
        );
        ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
        field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "New tag name".to_string()));
        if field.lost_focus()
            && ui.input(|i| i.key_pressed(Key::Enter))
            && !name.trim().is_empty()
            && tags.iter().all(|t| t.name != name.trim())
            && {
                let palette = suhina_library::store::default_tags();
                let color =
                    palette.get(tags.len() % palette.len().max(1)).map_or("#8C8A85".to_string(), |t| t.color.clone());
                run(projects, "library.setTag", json!({ "name": name.trim(), "color": color }), out)
            }
        {
            ui.ctx().data_mut(|d| d.remove::<String>(id));
            state.tag_counts = None;
            state.relist();
        }
    }
}
fn footer_view(
    ui: &mut Ui,
    area: Rect,
    state: &mut BrowserState,
    projects: &mut Projects,
    out: &mut BrowserOut,
) -> Rect {
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::panel_background());
    painter.hline(area.x_range(), area.top() + 0.5, theme::hairline(theme::border()));
    // The Edit tags button's room, so the dots stop before it.
    let edit_width = widgets::text_width(ui, "Edit tags", theme::ui_font(theme::TEXT_SMALL)) + FILTER_PAD * 2.0;
    let mut x = area.left() + PAD;
    // With the editor open, its rows are the tag list: the dots would say it twice.
    let tags = if !state.tag_editor { projects.file_library_mut().tags() } else { Vec::new() };
    for tag in tags {
        let rect = Rect::from_center_size(Pos2::new(x + DOT / 2.0 + 2.0, area.center().y), Vec2::splat(DOT + 4.0));
        x = rect.right() + 2.0;
        if x > area.right() - 90.0 - edit_width - 8.0 {
            break;
        }
        let response = widgets::button_area(ui, rect, Id::new(("browser-tag", &tag.name)), format!("Tag {}", tag.name))
            .on_hover_text(&tag.name);
        if response.hovered() {
            // The tag's name and how many items carry it, in the status bar. The count is read
            // where the tags are kept, not from the editor's `tag_counts`: that is absent until
            // the editor opens and stale after an item row's Tag menu changes a tag.
            let items = projects.file_library_mut().tagged_count(&tag.name);
            out.status =
                Some(Status::Hint(format!("Tag {} — {} item{}", tag.name, items, if items == 1 { "" } else { "s" })));
        }
        let on = state.tag_filter.contains(&tag.name);
        if response.clicked() {
            if on {
                state.tag_filter.retain(|t| *t != tag.name);
            } else {
                state.tag_filter.push(tag.name.clone());
            }
        }
        // Right-click: rename, recolour or remove the tag. Clicks inside do not close it: a
        // tag's new name is typed here (egui's default closes a menu on a click inside it).
        egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            let id = Id::new(("browser-rename-tag", &tag.name));
            let mut name: String = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| tag.name.clone());
            let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(120.0));
            ui.ctx().data_mut(|d| d.insert_temp(id, name.clone()));
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && name.trim() != tag.name {
                if run(projects, "library.setTag", json!({ "name": tag.name, "rename": name.trim() }), out) {
                    for filter in state.tag_filter.iter_mut().filter(|f| **f == tag.name) {
                        *filter = name.trim().to_string();
                    }
                }
                ui.ctx().data_mut(|d| d.remove::<String>(id));
                state.relist();
                ui.close();
            }
            // The palette track colours take, then any colour.
            let current = theme::parse_hex(&tag.color).map(crate::interaction::grid::to_rgb);
            if let Some(picked) = crate::interaction::colour_picker::swatch_row(ui, current) {
                let hex = theme::hex(crate::interaction::colour_picker::color32(picked));
                run(projects, "library.setTag", json!({ "name": tag.name, "color": hex }), out);
            }
            let mut color = tag_color(&tag.color);
            if egui::color_picker::color_edit_button_srgba(ui, &mut color, egui::color_picker::Alpha::Opaque).changed()
            {
                let hex = format!("#{:02X}{:02X}{:02X}", color.r(), color.g(), color.b());
                run(projects, "library.setTag", json!({ "name": tag.name, "color": hex }), out);
            }
            if ui.button("Remove tag").clicked() {
                if run(projects, "library.removeTag", json!({ "name": tag.name }), out) {
                    state.tag_filter.retain(|t| *t != tag.name);
                }
                state.relist();
                ui.close();
            }
        });
        painter.circle_filled(rect.center(), DOT / 2.0, tag_color(&tag.color));
        if on {
            painter.circle_stroke(rect.center(), DOT / 2.0 + 1.5, Stroke::new(1.0, theme::text_primary()));
        }
    }
    let count = match state.selected.len() {
        0 | 1 => format!("{} items", state.rows().iter().filter(|r| r.item.kind != ItemKind::Heading).count()),
        n => format!("{n} selected"),
    };
    // Left of the count: the button that opens and closes the tag editor; the popup anchors
    // to this rect, and a click outside the two closes the editor.
    let edit_rect = Rect::from_min_size(
        Pos2::new(area.right() - 96.0 - edit_width - 6.0, area.center().y - FILTER / 2.0),
        Vec2::new(edit_width, FILTER),
    );
    {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let rect = edit_rect;
        let response = widgets::button_area(ui, rect, Id::new("browser-edit-tags"), "Edit tags");
        if response.clicked() {
            state.tag_editor = !state.tag_editor;
            state.tag_counts = None;
            state.tag_editor_confirm = None;
        }
        let lit = motion::toward(ui.ctx(), response.id.with("on"), state.tag_editor, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, motion::mix(idle, theme::text_value(), lit));
        let text = motion::mix(theme::text_secondary(), theme::window_background(), lit);
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, "Edit tags", small.clone(), text);
    }
    widgets::text_in(
        &painter,
        area.shrink2(Vec2::new(PAD, 0.0)),
        Align2::RIGHT_CENTER,
        &count,
        theme::mono_font(theme::TEXT_TINY),
        theme::text_hint(),
    );
    edit_rect
}

/// The browser's resize handle at its inner edge: dragged, the width it is dragged to;
/// double-clicked, `default`. Gives that width, and whether the handle was let go (or reset)
/// now: the width is saved then, not on every frame of a drag.
pub fn resize_handle(ui: &Ui, handle: Rect, width: f32, left: bool, default: f32) -> (Option<f32>, bool) {
    let response = ui
        .interact(handle, Id::new("browser-resize"), Sense::click_and_drag())
        .on_hover_cursor(CursorIcon::ResizeHorizontal);
    if response.double_clicked() {
        return (Some(default), true);
    }
    ui.painter().rect_filled(
        handle,
        0.0,
        if response.hovered() || response.dragged() { theme::border_strong() } else { theme::border() },
    );
    // The width follows the pointer from where the drag began, so it never falls behind it
    // (adding each frame's movement to the width rounded to whole points would).
    let start_id = Id::new("browser-resize-start");
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
