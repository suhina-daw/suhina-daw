// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The start panel: what the window shows right of the browser while no project is open (at
//! start, as Settings › General › At start › Panel says, and after the last project closes). A
//! new project, one from a template, or a recent project, found by name and listed with when it
//! was last saved; the audio output at a glance; the version. Files dropped on it open as they
//! do anywhere on the window.

use crate::kit::surface::Backgrounds as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};

use crate::app::project_dialog::{BUTTON, LINE, Lists, heading, kind, row, scrolled, stem};
use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, button};

/// How wide the panel's content grows; the rest of the area stays empty around it.
const MAX_WIDTH: f32 = 820.0;
/// Below this width the audio card goes under the list instead of beside it.
const TWO_COLUMNS: f32 = 560.0;
const AUDIO_WIDTH: f32 = 240.0;
const ACTION: Vec2 = Vec2::new(132.0, 24.0);
const NOTICE_BUTTON: Vec2 = Vec2::new(150.0, 18.0);

/// Which list the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Recent,
    Templates,
}

/// What the panel keeps between frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartState {
    /// The text the list is filtered by.
    pub search: String,
    pub tab: Tab,
}

/// What a drop on the window opens, said on the start panel and in the status bar.
pub const DROP_HINT: &str = "Drop a project (.suhina) or an .als file here to open it";

/// The audio output as the panel shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioSummary {
    pub device: String,
    pub sample_rate: String,
    pub buffer: String,
}

/// What the user chose on the start panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartChoice {
    NewBlank,
    NewFromTemplate(PathBuf),
    Open(PathBuf),
    /// Open…: the system's dialog for a project to open.
    Browse,
    /// Settings › Audio.
    AudioSettings,
    /// Take a project off the recent list.
    Forget(PathBuf),
    /// The update line's button: Download update now, Restart now or Try again.
    Update,
    /// The update line's page (the release, or the pull request).
    UpdatePage(String),
}

/// The line under the actions about a newer build (`docs/spec/updates.md` › The start panel).
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub text: String,
    /// The button at its end.
    pub button: Option<&'static str>,
    /// How much of the download is done, 0 to 1, drawn under the text.
    pub progress: Option<f32>,
    /// The text says what went wrong.
    pub error: bool,
    /// The page *What's new* opens.
    pub page: Option<String>,
}

/// Draws the start panel in `area`; `now` dates the recent projects, `version` is the app's,
/// `notice` the line about a newer build.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    area: Rect,
    state: &mut StartState,
    lists: &Lists,
    audio: &AudioSummary,
    version: &str,
    notice: Option<&Notice>,
    now: SystemTime,
) -> Option<StartChoice> {
    // `.start-page` styles it; its buttons and fields take the sheet's `.button` and `.text-input`.
    let _style = crate::kit::style_class::enter("start-page");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    crate::kit::native_css::surface(&painter, area, 0.0, theme::window_background());
    let width = (area.width() - 4.0 * PAD).min(MAX_WIDTH);
    let content = Rect::from_min_max(
        Pos2::new(area.center().x - width / 2.0, area.top() + 4.0 * PAD),
        Pos2::new(area.center().x + width / 2.0, area.bottom() - 2.0 * PAD),
    );
    if content.width() <= 0.0 || content.height() <= 0.0 {
        return None;
    }
    let mut choice = None;

    // The actions along the top.
    let mut x = content.left();
    for (text, label, chosen) in [
        ("New project", "New project", Some(StartChoice::NewBlank)),
        ("From a template", "From a template", None),
        ("Open…", "Open a project", Some(StartChoice::Browse)),
    ] {
        let rect = Rect::from_min_size(Pos2::new(x, content.top()), ACTION);
        if rect.right() > content.right() {
            break;
        }
        if button(ui, &painter, rect, text, label) {
            match chosen {
                Some(chosen) => choice = Some(chosen),
                None => state.tab = Tab::Templates,
            }
        }
        x = rect.right() + PAD;
    }

    // What a drop does, right under the actions where it is read with them.
    let drop_hint = Rect::from_min_size(
        Pos2::new(content.left(), content.top() + ACTION.y + PAD),
        Vec2::new(content.width(), LINE),
    );
    widgets::text_in(
        &painter,
        drop_hint,
        Align2::LEFT_CENTER,
        DROP_HINT,
        theme::ui_font(theme::TEXT),
        theme::text_secondary(),
    );

    // The footer: the version.
    let footer = Rect::from_min_max(Pos2::new(content.left(), content.bottom() - LINE), content.max);
    let version = format!("Suhina {version}");
    widgets::text_in(
        &painter,
        footer,
        Align2::LEFT_CENTER,
        &version,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_hint(),
    );

    // A newer build, under the actions.
    let mut top = drop_hint.bottom() + PAD;
    if let Some(notice) = notice {
        let line = Rect::from_min_size(Pos2::new(content.left(), top), Vec2::new(content.width(), ACTION.y));
        if let Some(chosen) = notice_line(ui, &painter, line, notice) {
            choice = Some(chosen);
        }
        top = line.bottom() + 2.0 * PAD;
    }

    // The list, and the audio card beside it or under it.
    let body = Rect::from_min_max(Pos2::new(content.left(), top), Pos2::new(content.right(), footer.top() - PAD));
    let audio_height = 4.0 * LINE + BUTTON.y + 3.0 * PAD;
    let (list, card) = if body.width() >= TWO_COLUMNS {
        let card = Rect::from_min_size(
            Pos2::new(body.right() - AUDIO_WIDTH, body.top()),
            Vec2::new(AUDIO_WIDTH, audio_height.min(body.height())),
        );
        (Rect::from_min_max(body.min, Pos2::new(card.left() - 2.0 * PAD, body.bottom())), card)
    } else {
        let card = Rect::from_min_max(Pos2::new(body.left(), (body.bottom() - audio_height).max(body.top())), body.max);
        (Rect::from_min_max(body.min, Pos2::new(body.right(), card.top() - PAD)), card)
    };
    if let Some(chosen) = projects(ui, list, state, lists, now) {
        choice = Some(chosen);
    }
    if audio_card(ui, card, audio) {
        choice = Some(StartChoice::AudioSettings);
    }
    choice
}

/// The update line: its text, a progress bar while downloading, and its buttons on the right.
fn notice_line(ui: &Ui, painter: &egui::Painter, line: Rect, notice: &Notice) -> Option<StartChoice> {
    let mut choice = None;
    painter.fill_background(line, 0.0, theme::panel_background());
    let edge = if notice.error { theme::recording() } else { theme::accent() };
    painter.rect_filled(Rect::from_min_size(line.min, Vec2::new(3.0, line.height())), 0.0, edge);
    let mut right = line.right() - PAD;
    if let Some(text) = notice.button {
        let rect = Rect::from_min_size(
            Pos2::new(right - NOTICE_BUTTON.x, line.center().y - NOTICE_BUTTON.y / 2.0),
            NOTICE_BUTTON,
        );
        if button(ui, painter, rect, text, text) {
            choice = Some(StartChoice::Update);
        }
        right = rect.left() - PAD;
    }
    if let Some(page) = &notice.page {
        let rect = Rect::from_min_size(Pos2::new(right - BUTTON.x, line.center().y - BUTTON.y / 2.0), BUTTON);
        if button(ui, painter, rect, "What's new", "What's new in the new version") {
            choice = Some(StartChoice::UpdatePage(page.clone()));
        }
        right = rect.left() - PAD;
    }
    let text = Rect::from_min_max(Pos2::new(line.left() + 3.0 + PAD, line.top()), Pos2::new(right, line.bottom()));
    let colour = if notice.error { theme::recording() } else { theme::text_primary() };
    widgets::text_in(painter, text, Align2::LEFT_CENTER, &notice.text, theme::ui_font(theme::TEXT), colour);
    if let Some(part) = notice.progress {
        let bar = Rect::from_min_size(Pos2::new(text.left(), line.bottom() - 4.0), Vec2::new(text.width(), 2.0));
        painter.fill_background(bar, 0.0, theme::control_background());
        painter.rect_filled(
            Rect::from_min_size(bar.min, Vec2::new(bar.width() * part, bar.height())),
            0.0,
            theme::accent(),
        );
    }
    choice
}

/// The Recent and Templates tabs, the search field and the list.
fn projects(ui: &mut Ui, area: Rect, state: &mut StartState, lists: &Lists, now: SystemTime) -> Option<StartChoice> {
    if area.height() < 3.0 * LINE {
        return None;
    }
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::panel_background());
    painter.rect_stroke(area, 0.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    let inner = area.shrink(PAD);
    let mut choice = None;

    // Tabs on the left, the search field on the right.
    let mut x = inner.left();
    for (text, tab) in [("Recent", Tab::Recent), ("Templates", Tab::Templates)] {
        let font = theme::ui_font(theme::TEXT);
        let w = widgets::text_width(ui, text, font.clone()) + 2.0 * PAD;
        let rect = Rect::from_min_size(Pos2::new(x, inner.top()), Vec2::new(w, LINE));
        let response = widgets::button_area(ui, rect, egui::Id::new(("start-tab", text)), format!("{text} projects"));
        let on = state.tab == tab;
        let color = if on { theme::text_primary() } else { theme::text_hint() };
        widgets::text_in(&painter, rect, Align2::CENTER_CENTER, text, font, color);
        if on {
            painter.hline(rect.x_range(), rect.bottom() - 1.0, egui::Stroke::new(2.0, theme::accent()));
        }
        if response.clicked() {
            state.tab = tab;
        }
        x = rect.right() + PAD;
    }
    let search_width = (inner.right() - x - PAD).min(200.0);
    if search_width > 60.0 {
        let field =
            Rect::from_min_size(Pos2::new(inner.right() - search_width, inner.top()), Vec2::new(search_width, LINE));
        let edit = crate::kit::widgets::text_input_at(
            ui,
            field,
            egui::TextEdit::singleline(&mut state.search).hint_text("Search").font(theme::ui_font(theme::TEXT_SMALL)),
        );
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search projects"));
    }
    let top = inner.top() + LINE + PAD;
    painter.hline(inner.x_range(), top - PAD / 2.0, theme::hairline(theme::border()));
    let list = Rect::from_min_max(Pos2::new(inner.left(), top), inner.max);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let search = state.search.trim().to_lowercase();
    let found = |path: &Path| search.is_empty() || stem(path).to_lowercase().contains(&search);

    match state.tab {
        Tab::Recent => {
            let rows: Vec<(&PathBuf, Option<SystemTime>)> = lists
                .recent
                .iter()
                .enumerate()
                .filter(|(_, p)| found(p))
                .map(|(i, p)| (p, lists.saved.get(i).copied().flatten()))
                .collect();
            if rows.is_empty() {
                let text = if lists.recent.is_empty() {
                    "No projects yet: start a new one, or drop a project here"
                } else {
                    "No recent project has that name"
                };
                let empty = Rect::from_min_size(list.min, Vec2::new(list.width(), LINE));
                widgets::text_in(&painter, empty, Align2::LEFT_CENTER, text, small, theme::text_hint());
            } else {
                scrolled(ui, list, "start-recent", |ui| {
                    for (path, saved) in rows {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
                        let painter = ui.painter().clone();
                        let note = saved.map_or_else(
                            || path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
                            |saved| ago(now, saved),
                        );
                        let label = format!("Open {}", stem(path));
                        let response =
                            row(ui, &painter, r, &stem(path), &note, &label).on_hover_text(path.display().to_string());
                        if response.clicked() {
                            choice = Some(StartChoice::Open(path.clone()));
                        }
                        response.context_menu(|ui| {
                            if ui.button("Remove from list").clicked() {
                                choice = Some(StartChoice::Forget(path.clone()));
                                ui.close();
                            }
                        });
                    }
                });
            }
        }
        Tab::Templates => {
            let rows: Vec<&PathBuf> = lists.templates.iter().filter(|p| found(p)).collect();
            let note = Rect::from_min_max(Pos2::new(list.left(), list.bottom() - LINE), list.max);
            let folder = format!("Templates in {}; the folder is set in Settings › General", lists.templates_folder);
            widgets::text_in(&painter, note, Align2::LEFT_CENTER, &folder, small.clone(), theme::text_hint());
            let list = Rect::from_min_max(list.min, Pos2::new(list.right(), note.top() - 4.0));
            if rows.is_empty() {
                let text = if lists.templates.is_empty() {
                    "No templates yet: File › Save as template"
                } else {
                    "No template has that name"
                };
                let empty = Rect::from_min_size(list.min, Vec2::new(list.width(), LINE));
                widgets::text_in(&painter, empty, Align2::LEFT_CENTER, text, small, theme::text_hint());
            } else {
                scrolled(ui, list, "start-templates", |ui| {
                    for path in rows {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
                        let painter = ui.painter().clone();
                        let label = format!("Start from {}", stem(path));
                        if row(ui, &painter, r, &stem(path), &kind(path), &label).clicked() {
                            choice = Some(StartChoice::NewFromTemplate(path.clone()));
                        }
                    }
                });
            }
        }
    }
    choice
}

/// The output device, rate and buffer, and a button to Settings › Audio; true when it is clicked.
fn audio_card(ui: &Ui, area: Rect, audio: &AudioSummary) -> bool {
    if area.height() < 3.0 * LINE {
        return false;
    }
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::panel_background());
    painter.rect_stroke(area, 0.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    let inner = area.shrink(PAD);
    heading(&painter, Rect::from_min_size(inner.min, Vec2::new(inner.width(), 0.0)), "Audio");
    let small = theme::ui_font(theme::TEXT_SMALL);
    let mut y = inner.top() + LINE;
    for (name, value) in [("Output", &audio.device), ("Sample rate", &audio.sample_rate), ("Buffer", &audio.buffer)] {
        let r = Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width(), LINE));
        widgets::text_in(&painter, r, Align2::LEFT_CENTER, name, small.clone(), theme::text_secondary());
        let value_rect = Rect::from_min_max(Pos2::new(r.left() + 76.0, r.top()), r.max);
        widgets::text_in(&painter, value_rect, Align2::LEFT_CENTER, value, small.clone(), theme::text_primary());
        y += LINE;
    }
    let settings = Rect::from_min_size(Pos2::new(inner.left(), (y + PAD).min(inner.bottom() - BUTTON.y)), {
        Vec2::new(BUTTON.x + 24.0, BUTTON.y)
    });
    button(ui, &painter, settings, "Audio settings…", "Audio settings")
}

/// How long ago `then` was, in words: "Just now", "5 minutes ago", "Yesterday", "3 weeks ago".
pub fn ago(now: SystemTime, then: SystemTime) -> String {
    let seconds = now.duration_since(then).unwrap_or(Duration::ZERO).as_secs();
    let (minute, hour, day) = (60, 3600, 86_400);
    let count = |n: u64, one: &str, many: &str| if n == 1 { one.to_string() } else { format!("{n} {many} ago") };
    match seconds {
        s if s < minute => "Just now".into(),
        s if s < hour => count(s / minute, "A minute ago", "minutes"),
        s if s < day => count(s / hour, "An hour ago", "hours"),
        s if s < 2 * day => "Yesterday".into(),
        s if s < 7 * day => count(s / day, "Yesterday", "days"),
        s if s < 30 * day => count(s / (7 * day), "Last week", "weeks"),
        s if s < 365 * day => count(s / (30 * day), "Last month", "months"),
        s => count(s / (365 * day), "Last year", "years"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    type Panel = (StartState, Option<StartChoice>);

    fn panel(lists: Lists, size: Vec2) -> Harness<'static, Panel> {
        panel_with(lists, size, None)
    }

    fn panel_with(lists: Lists, size: Vec2, notice: Option<Notice>) -> Harness<'static, Panel> {
        let audio = AudioSummary { device: "Speakers".into(), sample_rate: "48000 Hz".into(), buffer: "256".into() };
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        Harness::builder().with_size(size).build_ui_state(
            move |ui, (state, chosen): &mut Panel| {
                let area = ui.max_rect();
                if let Some(choice) = show(ui, area, state, &lists, &audio, "0.26.0", notice.as_ref(), now) {
                    *chosen = Some(choice);
                }
            },
            (StartState::default(), None),
        )
    }

    fn lists() -> Lists {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        Lists {
            recent: vec![PathBuf::from("songs/Night drive.suhina"), PathBuf::from("songs/Morning.suhina")],
            saved: vec![Some(now - Duration::from_secs(3 * 86_400)), None],
            templates: vec![PathBuf::from("templates/Beat.suhina")],
            templates_folder: "templates".into(),
        }
    }

    #[test]
    fn the_panel_makes_new_projects_and_opens_recent_ones() {
        let mut harness = panel(lists(), Vec2::new(900.0, 600.0));
        harness.run();
        harness.get_by_label("New project").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::NewBlank));
        harness.get_by_label("Open Night drive").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::Open("songs/Night drive.suhina".into())));
        harness.get_by_label("Open a project").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::Browse));
        harness.get_by_label("Audio settings").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::AudioSettings));
        // From a template shows the templates; one of them starts a new project.
        harness.get_by_label("From a template").click();
        harness.run();
        assert_eq!(harness.state().0.tab, Tab::Templates);
        harness.get_by_label("Start from Beat").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::NewFromTemplate("templates/Beat.suhina".into())));
        harness.get_by_label("Recent projects").click();
        harness.run();
        assert_eq!(harness.state().0.tab, Tab::Recent);
    }

    #[test]
    fn a_newer_build_is_offered_under_the_actions() {
        let notice = Notice {
            text: "New version available: Suhina 0.0.1-nightly.20261018+c176a33".into(),
            button: Some("Download update now"),
            progress: None,
            error: false,
            page: Some("https://example.invalid/nightly".into()),
        };
        let mut harness = panel_with(lists(), Vec2::new(900.0, 600.0), Some(notice));
        harness.run();
        assert!(harness.query_by_label("Open Night drive").is_some());
        harness.get_by_label("Download update now").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::Update));
        harness.get_by_label("What's new in the new version").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::UpdatePage("https://example.invalid/nightly".into())));
    }

    #[test]
    fn the_search_field_filters_the_list_by_name() {
        let mut harness = panel(lists(), Vec2::new(900.0, 600.0));
        harness.run();
        assert!(harness.query_by_label("Open Morning").is_some());
        harness.get_by_label("Search projects").click();
        harness.run();
        harness.get_by_label("Search projects").type_text("NIGHT");
        harness.run();
        assert!(harness.query_by_label("Open Night drive").is_some());
        assert!(harness.query_by_label("Open Morning").is_none());
    }

    #[test]
    fn a_recent_project_is_taken_off_the_list_from_its_menu() {
        let mut harness = panel(lists(), Vec2::new(900.0, 600.0));
        harness.run();
        harness.get_by_label("Open Morning").click_secondary();
        harness.run();
        harness.get_by_label("Remove from list").click();
        harness.run();
        assert_eq!(harness.state().1, Some(StartChoice::Forget("songs/Morning.suhina".into())));
    }

    #[test]
    fn the_panel_fits_a_narrow_window_and_lists_nothing_without_projects() {
        let mut harness = panel(Lists::default(), Vec2::new(420.0, 520.0));
        harness.run();
        assert!(harness.query_by_label("Audio settings").is_some(), "the audio card goes under the list");
        assert!(harness.query_by_label("New project").is_some());
    }

    #[test]
    fn ages_read_in_words() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let before = |s: u64| ago(now, now - Duration::from_secs(s));
        assert_eq!(before(10), "Just now");
        assert_eq!(before(60), "A minute ago");
        assert_eq!(before(5 * 60), "5 minutes ago");
        assert_eq!(before(3600), "An hour ago");
        assert_eq!(before(5 * 3600), "5 hours ago");
        assert_eq!(before(30 * 3600), "Yesterday");
        assert_eq!(before(3 * 86_400), "3 days ago");
        assert_eq!(before(8 * 86_400), "Last week");
        assert_eq!(before(15 * 86_400), "2 weeks ago");
        assert_eq!(before(40 * 86_400), "Last month");
        assert_eq!(before(100 * 86_400), "3 months ago");
        assert_eq!(before(400 * 86_400), "Last year");
        assert_eq!(before(800 * 86_400), "2 years ago");
        // A file saved in the future (a clock set wrong) is just now.
        assert_eq!(ago(now, now + Duration::from_secs(100)), "Just now");
    }
}
