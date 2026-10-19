// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The project dialog (File › New or open, Ctrl+N): a new blank project, a new one from a
//! template, or a project to open, typed as a path or picked from the recent ones. And the save
//! prompt, which asks for a name and a folder when a project is saved for the first time, saved
//! under another name, or saved as a template. And the question asked before a project with
//! unsaved changes is closed, replaced or the app quits. And the offer, on start, of a backup a
//! crash left (`docs/spec/backups.md`). And Help › About Suhina, which says which build this is
//! (docs/spec/about.md). All are drawn over the window; Escape closes them.

use std::path::{Path, PathBuf};

use egui::{Align2, Id, Key, Pos2, Rect, Sense, Ui, Vec2};

use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, button};

const WIDTH: f32 = 620.0;
const HEIGHT: f32 = 380.0;
const SAVE_WIDTH: f32 = 420.0;
pub(crate) const LINE: f32 = 20.0;
pub(crate) const BUTTON: Vec2 = Vec2::new(96.0, 18.0);

/// What the project dialog keeps between frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogState {
    /// The path typed into the open field.
    pub path: String,
    /// The open field takes the keys when the dialog appears (File › Open, Ctrl+O).
    pub focus_path: bool,
}

/// What the dialog lists: the recent projects, the templates and where they are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lists {
    pub recent: Vec<PathBuf>,
    /// When each recent project was last saved, where it could be read.
    pub saved: Vec<Option<std::time::SystemTime>>,
    pub templates: Vec<PathBuf>,
    pub templates_folder: String,
}

/// What the user chose in the project dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Open(PathBuf),
    NewBlank,
    NewFromTemplate(PathBuf),
    /// Browse…: the system's dialog for a project to open.
    Browse,
    Close,
}

/// Draws the project dialog over `area`, above everything else.
pub fn show(ui: &Ui, area: Rect, state: &mut DialogState, lists: &Lists) -> Option<Choice> {
    egui::Area::new(Id::new("project-dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw(ui, area, state, lists)
        })
        .inner
}

fn draw(ui: &mut Ui, area: Rect, state: &mut DialogState, lists: &Lists) -> Option<Choice> {
    let mut choice = None;
    let panel = panel(ui, area, Vec2::new(WIDTH, HEIGHT), "New or open a project", &mut choice);
    let painter = ui.painter_at(area);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let half = (panel.width() - 3.0 * PAD) / 2.0;
    let left = Rect::from_min_size(Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD), Vec2::new(half, 0.0));
    let right = Rect::from_min_size(Pos2::new(left.right() + PAD, left.top()), Vec2::new(half, 0.0));
    let bottom = panel.bottom() - PAD;

    // New: blank, or from a template.
    heading(&painter, left, "New");
    let mut y = left.top() + LINE;
    let blank = Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(half, LINE));
    if row(ui, &painter, blank, "New blank project", "4 tracks, 2 returns, 8 scenes", "New blank project").clicked() {
        choice = Some(Choice::NewBlank);
    }
    y = blank.bottom() + PAD;
    heading(&painter, Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(half, 0.0)), "From a template");
    y += LINE;
    let note = Rect::from_min_max(Pos2::new(left.left(), bottom - 28.0), Pos2::new(left.right(), bottom));
    let list = Rect::from_min_max(Pos2::new(left.left(), y), Pos2::new(left.right(), note.top() - 4.0));
    if lists.templates.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(half, LINE));
        let text = "No templates yet: File › Save as template";
        widgets::text_in(&painter, empty, Align2::LEFT_CENTER, text, small.clone(), theme::text_hint());
    } else {
        scrolled(ui, list, "templates", |ui| {
            for path in &lists.templates {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
                let painter = ui.painter().clone();
                let label = format!("Template {}", stem(path));
                if row(ui, &painter, r, &stem(path), &kind(path), &label).clicked() {
                    choice = Some(Choice::NewFromTemplate(path.clone()));
                }
            }
        });
    }
    let folder = format!("Templates in {}; the folder is set in Settings › General", lists.templates_folder);
    let lines = widgets::fitted(&painter, &folder, small.clone(), theme::text_hint(), note.width());
    painter.galley(Pos2::new(note.left(), note.center().y - lines.size().y / 2.0), lines, theme::text_hint());

    // Open: a path, or a recent project.
    heading(&painter, right, "Open");
    let y = right.top() + LINE;
    let open_button = Rect::from_min_size(Pos2::new(right.right() - 56.0, y + 1.0), Vec2::new(56.0, BUTTON.y));
    let browse = Rect::from_min_size(Pos2::new(open_button.left() - 4.0 - 64.0, y + 1.0), Vec2::new(64.0, BUTTON.y));
    if button(ui, &painter, browse, "Browse…", "Browse for a project") {
        choice = Some(Choice::Browse);
    }
    let field = Rect::from_min_max(Pos2::new(right.left(), y), Pos2::new(browse.left() - 4.0, y + 20.0));
    let edit = ui.put(
        field,
        egui::TextEdit::singleline(&mut state.path).hint_text("Path to a .suhina or .als file").font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Project path"));
    if std::mem::take(&mut state.focus_path) {
        edit.request_focus();
    }
    let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if button(ui, &painter, open_button, "Open", "Open the project at the path") || entered {
        let path = state.path.trim().trim_matches('"');
        if !path.is_empty() {
            choice = Some(Choice::Open(PathBuf::from(path)));
        }
    }
    let recent_top = field.bottom() + PAD;
    heading(&painter, Rect::from_min_size(Pos2::new(right.left(), recent_top), Vec2::new(half, 0.0)), "Recent");
    let list = Rect::from_min_max(Pos2::new(right.left(), recent_top + LINE), Pos2::new(right.right(), bottom));
    if lists.recent.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(half, LINE));
        widgets::text_in(&painter, empty, Align2::LEFT_CENTER, "Nothing opened yet", small, theme::text_hint());
    } else {
        scrolled(ui, list, "recent", |ui| {
            for path in &lists.recent {
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
                let painter = ui.painter().clone();
                let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                let label = format!("Recent {}", stem(path));
                if row(ui, &painter, r, &stem(path), &folder, &label).clicked() {
                    choice = Some(Choice::Open(path.clone()));
                }
            }
        });
    }
    if choice.is_none() && ui.input(|i| i.key_pressed(Key::Escape)) {
        choice = Some(Choice::Close);
    }
    choice
}

/// What the save prompt keeps between frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavePrompt {
    /// A template is a copy in the templates folder; the project stays with its own file.
    pub template: bool,
    pub name: String,
    pub folder: String,
    /// The name field takes the keys when the prompt appears.
    pub focus: bool,
    /// Collect and save: the audio from outside the folder is copied into it first.
    pub collect: bool,
}

/// What the user did with the save prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveChoice {
    Save(PathBuf),
    /// Browse… beside the folder: the system's dialog for it; the prompt stays.
    BrowseFolder,
    Cancel,
}

/// Draws the save prompt over `area`.
pub fn show_save(ui: &Ui, area: Rect, prompt: &mut SavePrompt) -> Option<SaveChoice> {
    egui::Area::new(Id::new("save-prompt"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_save(ui, area, prompt)
        })
        .inner
}

fn draw_save(ui: &mut Ui, area: Rect, prompt: &mut SavePrompt) -> Option<SaveChoice> {
    let mut closed: Option<SaveChoice> = None;
    let title = if prompt.template { "Save as template" } else { "Save project" };
    let panel = panel(ui, area, Vec2::new(SAVE_WIDTH, ROW + 4.0 * LINE + 2.0 * PAD), title, &mut closed);
    let painter = ui.painter_at(area);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let label_width = 52.0;
    let mut y = panel.top() + ROW + PAD;
    let mut browse_folder = false;
    let mut field = |ui: &mut Ui, label: &str, value: &mut String, focus: bool| {
        let r = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
        widgets::text_in(&painter, r, Align2::LEFT_CENTER, label, small.clone(), theme::text_secondary());
        let mut input = Rect::from_min_max(Pos2::new(r.left() + label_width, r.top()), r.max);
        if label == "Folder" {
            let browse = Rect::from_min_size(Pos2::new(r.right() - 64.0, r.top() + 1.0), Vec2::new(64.0, BUTTON.y));
            browse_folder = button(ui, &painter, browse, "Browse…", "Browse for the folder");
            input.max.x = browse.left() - 4.0;
        }
        let edit = ui.put(input, egui::TextEdit::singleline(value).font(small.clone()));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
        if focus {
            edit.request_focus();
        }
        y += LINE + 4.0;
        edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
    };
    let focus = std::mem::take(&mut prompt.focus);
    let mut entered = field(ui, "Name", &mut prompt.name, focus);
    entered |= field(ui, "Folder", &mut prompt.folder, false);
    let name = prompt.name.trim();
    let path = (!name.is_empty() && !name.contains(['/', '\\', ':']))
        .then(|| Path::new(prompt.folder.trim()).join(format!("{name}.{}", suhina_session::PROJECT_EXTENSION)));
    let note = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
    let text = match &path {
        Some(path) if path.exists() => format!("{} is there already and will be replaced", path.display()),
        Some(path) => path.display().to_string(),
        None => "A name without / \\ or :".to_string(),
    };
    let color = if path.as_ref().is_some_and(|p| p.exists()) { theme::recording() } else { theme::text_hint() };
    widgets::text_in(&painter, note, Align2::LEFT_CENTER, &text, small, color);
    let save = Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, panel.bottom() - PAD - BUTTON.y), BUTTON);
    let cancel = Rect::from_min_size(Pos2::new(save.left() - 4.0 - BUTTON.x, save.top()), BUTTON);
    let saved = button(ui, &painter, save, "Save", "Save");
    if button(ui, &painter, cancel, "Cancel", "Cancel saving") || ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(SaveChoice::Cancel);
    }
    if closed.is_some() {
        return Some(SaveChoice::Cancel);
    }
    if browse_folder {
        return Some(SaveChoice::BrowseFolder);
    }
    match path {
        Some(path) if saved || entered => Some(SaveChoice::Save(path)),
        _ => None,
    }
}

/// What the user answered when asked about a project's unsaved changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsavedChoice {
    Save,
    DontSave,
    Cancel,
}

/// Asks whether to save `name`'s changes before `doing` ("closing it", "quitting"), over `area`.
pub fn show_unsaved(ui: &Ui, area: Rect, name: &str, doing: &str) -> Option<UnsavedChoice> {
    egui::Area::new(Id::new("unsaved-prompt"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_unsaved(ui, area, name, doing)
        })
        .inner
}

/// The version of this build: the workspace version, which a nightly sets to its own,
/// `0.0.1-nightly.20261013+a15ad35` (docs/releasing.md › Nightly).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What this build was, from `build.rs`: `2026_10_16_a15ad35`, the checked-out commit's date and
/// abbreviated hash; a build with no git says `unknown` unless the release set the environment.
pub const BUILD: &str = env!("SUHINA_BUILD");

/// What was chosen in the About dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AboutChoice {
    /// Copy [`version_line`] to the clipboard; the dialog stays open.
    Copy,
    Close,
}

/// One line that names this build, to paste into a bug report:
/// `Suhina 0.0.1 build 2026_10_16_a15ad35 (linux x86_64)`.
pub fn version_line() -> String {
    format!("Suhina {VERSION} build {BUILD} ({})", platform())
}

fn platform() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Draws the About dialog over `area`. Returns what was chosen, or `None` while it stays open.
pub fn show_about(ui: &Ui, area: Rect) -> Option<AboutChoice> {
    egui::Area::new(Id::new("about-dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_about(ui, area)
        })
        .inner
}

fn draw_about(ui: &mut Ui, area: Rect) -> Option<AboutChoice> {
    let mut closed: Option<AboutChoice> = None;
    let platform = platform();
    let facts = [
        ("Version", VERSION),
        ("Build", BUILD),
        ("Platform", platform.as_str()),
        ("Licence", "GPL-3.0-or-later, free software"),
    ];
    let height = ROW + facts.len() as f32 * LINE + BUTTON.y + 3.0 * PAD;
    let panel = panel(ui, area, Vec2::new(SAVE_WIDTH, height), "About Suhina", &mut closed);
    let painter = ui.painter_at(area);
    let font = theme::ui_font(theme::TEXT);
    for (i, (name, value)) in facts.iter().enumerate() {
        let line = Rect::from_min_size(
            Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD + i as f32 * LINE),
            Vec2::new(panel.width() - 2.0 * PAD, LINE),
        );
        widgets::text_in(&painter, line, Align2::LEFT_CENTER, name, font.clone(), theme::text_hint());
        let value_at = Rect::from_min_max(Pos2::new(line.left() + 80.0, line.top()), line.max);
        widgets::text_in(&painter, value_at, Align2::LEFT_CENTER, value, font.clone(), theme::text_primary());
    }
    let copy = Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, panel.bottom() - PAD - BUTTON.y), BUTTON);
    // The label carries the line itself, so a screen reader reads the version too.
    if button(ui, &painter, copy, "Copy version", &format!("Copy version: {}", version_line())) {
        return Some(AboutChoice::Copy);
    }
    if ui.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Enter)) {
        return Some(AboutChoice::Close);
    }
    closed
}

fn draw_unsaved(ui: &mut Ui, area: Rect, name: &str, doing: &str) -> Option<UnsavedChoice> {
    let mut closed: Option<UnsavedChoice> = None;
    let panel =
        panel(ui, area, Vec2::new(SAVE_WIDTH, ROW + 2.0 * LINE + BUTTON.y + 3.0 * PAD), "Unsaved changes", &mut closed);
    let painter = ui.painter_at(area);
    let text = Rect::from_min_size(
        Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD),
        Vec2::new(panel.width() - 2.0 * PAD, LINE),
    );
    let message = format!("Save the changes to {name} before {doing}?");
    widgets::text_in(&painter, text, Align2::LEFT_CENTER, &message, theme::ui_font(theme::TEXT), theme::text_primary());
    let save = Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, panel.bottom() - PAD - BUTTON.y), BUTTON);
    let dont = Rect::from_min_size(Pos2::new(save.left() - 4.0 - BUTTON.x, save.top()), BUTTON);
    let cancel = Rect::from_min_size(Pos2::new(dont.left() - 4.0 - BUTTON.x, save.top()), BUTTON);
    if button(ui, &painter, save, "Save", "Save the changes") || ui.input(|i| i.key_pressed(Key::Enter)) {
        return Some(UnsavedChoice::Save);
    }
    if button(ui, &painter, dont, "Don't save", "Don't save the changes") {
        return Some(UnsavedChoice::DontSave);
    }
    if button(ui, &painter, cancel, "Cancel", "Cancel and keep the project open")
        || ui.input(|i| i.key_pressed(Key::Escape))
    {
        return Some(UnsavedChoice::Cancel);
    }
    closed
}

/// What the user chose for a backup offered on start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryChoice {
    /// Open it as an unsaved project tied to its project file.
    Open,
    /// Show it in the file manager; the offer stays.
    ShowFolder,
    /// Leave it in its folder and do not offer it again.
    Ignore,
}

const RECOVERY_WIDTH: f32 = 520.0;
const RECOVERY_BUTTON: Vec2 = Vec2::new(120.0, 18.0);

/// Offers the backup of `name` written at `written` (local time) that is newer than its project
/// file, over `area`; `saved` says whether the project has a file at all.
pub fn show_recovery(ui: &Ui, area: Rect, name: &str, written: &str, saved: bool) -> Option<RecoveryChoice> {
    egui::Area::new(Id::new("recovery-prompt"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_recovery(ui, area, name, written, saved)
        })
        .inner
}

fn draw_recovery(ui: &mut Ui, area: Rect, name: &str, written: &str, saved: bool) -> Option<RecoveryChoice> {
    let mut closed: Option<RecoveryChoice> = None;
    let size = Vec2::new(RECOVERY_WIDTH, ROW + 3.0 * LINE + RECOVERY_BUTTON.y + 3.0 * PAD);
    let panel = panel(ui, area, size, "Backup found", &mut closed);
    let painter = ui.painter_at(area);
    let lines = [
        format!("Suhina closed before {name} was saved."),
        if saved {
            format!("A backup from {written} is newer than the project file.")
        } else {
            format!("A backup from {written} is all there is of it.")
        },
    ];
    for (n, line) in lines.iter().enumerate() {
        let text = Rect::from_min_size(
            Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD + n as f32 * LINE),
            Vec2::new(panel.width() - 2.0 * PAD, LINE),
        );
        widgets::text_in(&painter, text, Align2::LEFT_CENTER, line, theme::ui_font(theme::TEXT), theme::text_primary());
    }
    let bottom = panel.bottom() - PAD - RECOVERY_BUTTON.y;
    let open = Rect::from_min_size(Pos2::new(panel.right() - PAD - RECOVERY_BUTTON.x, bottom), RECOVERY_BUTTON);
    let folder = Rect::from_min_size(Pos2::new(open.left() - 4.0 - RECOVERY_BUTTON.x, bottom), RECOVERY_BUTTON);
    let ignore = Rect::from_min_size(Pos2::new(folder.left() - 4.0 - RECOVERY_BUTTON.x, bottom), RECOVERY_BUTTON);
    if button(ui, &painter, open, "Open the backup", "Open the backup") || ui.input(|i| i.key_pressed(Key::Enter)) {
        return Some(RecoveryChoice::Open);
    }
    if button(ui, &painter, folder, "Show the folder", "Show the backup's folder") {
        return Some(RecoveryChoice::ShowFolder);
    }
    if button(ui, &painter, ignore, "Ignore", "Ignore the backup") || ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(RecoveryChoice::Ignore);
    }
    closed
}

/// The person's answer to an agent that connected (ADR 0028).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentChoice {
    Allow,
    Deny,
}

/// Asks whether to let `client`, an agent that has just connected, work on the open projects,
/// over `area`.
pub fn show_agent(ui: &Ui, area: Rect, client: &str) -> Option<AgentChoice> {
    egui::Area::new(Id::new("agent-prompt"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_agent(ui, area, client)
        })
        .inner
}

fn draw_agent(ui: &mut Ui, area: Rect, client: &str) -> Option<AgentChoice> {
    let mut closed: Option<AgentChoice> = None;
    let size = Vec2::new(RECOVERY_WIDTH, ROW + 3.0 * LINE + BUTTON.y + 3.0 * PAD);
    let panel = panel(ui, area, size, "An agent wants to connect", &mut closed);
    let painter = ui.painter_at(area);
    let lines = [
        format!("{client} wants to work on your open projects."),
        "You see every change it makes and can undo it; it cannot close tabs or change settings.".to_string(),
        "The name is what the agent says it is.".to_string(),
    ];
    for (n, line) in lines.iter().enumerate() {
        let text = Rect::from_min_size(
            Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD + n as f32 * LINE),
            Vec2::new(panel.width() - 2.0 * PAD, LINE),
        );
        let colour = if n == 0 { theme::text_primary() } else { theme::text_hint() };
        widgets::text_in(&painter, text, Align2::LEFT_CENTER, line, theme::ui_font(theme::TEXT), colour);
    }
    let bottom = panel.bottom() - PAD - BUTTON.y;
    let allow = Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, bottom), BUTTON);
    let deny = Rect::from_min_size(Pos2::new(allow.left() - 4.0 - BUTTON.x, bottom), BUTTON);
    if button(ui, &painter, allow, "Allow", &format!("Allow {client}")) {
        return Some(AgentChoice::Allow);
    }
    // Enter does not allow: letting an agent in takes a click.
    if button(ui, &painter, deny, "Deny", &format!("Deny {client}")) || ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(AgentChoice::Deny);
    }
    closed
}

/// A panel in the middle of `area` with a title row and a Close button, the area behind dimmed
/// and its clicks swallowed. Returns the panel; `closed` is set when Close is clicked.
pub(crate) fn panel<T: CloseChoice>(ui: &Ui, area: Rect, size: Vec2, title: &str, closed: &mut Option<T>) -> Rect {
    let behind = ui.interact(area, Id::new(("dialog-behind", title)), Sense::click_and_drag());
    let _swallowed = behind.clicked();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(0.6));
    let size = Vec2::new(size.x.min(area.width() - 2.0 * PAD), size.y.min(area.height() - 2.0 * PAD));
    let rect = Rect::from_center_size(area.center(), size);
    painter.rect_filled(rect, 0.0, theme::panel_background());
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(bar, 0.0, theme::control_background());
    let font = theme::ui_font(theme::TEXT);
    widgets::text_in(
        &painter,
        bar.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        title,
        font,
        theme::text_primary(),
    );
    let close = Rect::from_min_size(Pos2::new(bar.right() - 52.0, bar.top()), Vec2::new(52.0, ROW));
    let response = widgets::button_area(ui, close, Id::new(("dialog-close", title)), format!("Close {title}"));
    let hover = widgets::hover(ui, &response);
    painter.rect_filled(close, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    let small = theme::ui_font(theme::TEXT_SMALL);
    widgets::text_in(&painter, close, Align2::CENTER_CENTER, "Close", small, theme::text_primary());
    if response.clicked() {
        *closed = Some(T::close());
    }
    rect
}

/// The choice that closes a dialog.
pub(crate) trait CloseChoice {
    fn close() -> Self;
}

impl CloseChoice for Choice {
    fn close() -> Self {
        Choice::Close
    }
}

impl CloseChoice for SaveChoice {
    fn close() -> Self {
        SaveChoice::Cancel
    }
}

impl CloseChoice for RecoveryChoice {
    fn close() -> Self {
        RecoveryChoice::Ignore
    }
}

impl CloseChoice for AboutChoice {
    fn close() -> Self {
        AboutChoice::Close
    }
}

impl CloseChoice for AgentChoice {
    fn close() -> Self {
        AgentChoice::Deny
    }
}

impl CloseChoice for UnsavedChoice {
    fn close() -> Self {
        UnsavedChoice::Cancel
    }
}

pub(crate) fn heading(painter: &egui::Painter, at: Rect, text: &str) {
    let r = Rect::from_min_size(at.min, Vec2::new(at.width(), LINE));
    widgets::text_in(painter, r, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), theme::text_hint());
    painter.hline(r.x_range(), r.bottom() - 2.5, theme::hairline(theme::border()));
}

/// A row to click: a name, and a note on the right.
pub(crate) fn row(ui: &Ui, painter: &egui::Painter, rect: Rect, name: &str, note: &str, label: &str) -> egui::Response {
    let response = widgets::button_area(ui, rect, Id::new(("dialog-row", label)), label);
    let fill = motion::mix(theme::panel_background(), theme::panel_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 0.0, fill);
    let inner = rect.shrink2(Vec2::new(PAD, 0.0));
    let font = theme::ui_font(theme::TEXT);
    let name_width = widgets::text_width(ui, name, font.clone()).min(inner.width() * 0.6);
    widgets::text_in(painter, inner, Align2::LEFT_CENTER, name, font, theme::text_primary());
    let rest = Rect::from_min_max(Pos2::new(inner.left() + name_width + 8.0, inner.top()), inner.max);
    let small = theme::ui_font(theme::TEXT_SMALL);
    widgets::text_in(painter, rest, Align2::RIGHT_CENTER, note, small, theme::text_hint());
    response
}

pub(crate) fn scrolled(ui: &mut Ui, rect: Rect, salt: &str, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
    });
}

pub(crate) fn stem(path: &Path) -> String {
    path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

/// What a template file is, by its extension.
pub(crate) fn kind(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("als") => "Live Set".into(),
        _ => "Suhina project".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn dialog(lists: Lists) -> Harness<'static, (DialogState, Option<Choice>)> {
        Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
            move |ui, (state, chosen): &mut (DialogState, Option<Choice>)| {
                let area = ui.max_rect();
                if let Some(choice) = show(ui, area, state, &lists) {
                    *chosen = Some(choice);
                }
            },
            (DialogState::default(), None),
        )
    }

    #[test]
    fn the_dialog_makes_new_projects_and_opens_recent_ones() {
        let lists = Lists {
            recent: vec![PathBuf::from("songs/Night.suhina")],
            templates: vec![
                PathBuf::from("templates/Beat.suhina"),
                PathBuf::from("templates/Groove.suhina"),
                PathBuf::from("templates/Old.als"),
            ],
            templates_folder: "templates".into(),
            ..Lists::default()
        };
        let mut harness = dialog(lists);
        harness.run();
        harness.get_by_label("New blank project").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::NewBlank));
        harness.get_by_label("Template Beat").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::NewFromTemplate("templates/Beat.suhina".into())));
        harness.get_by_label("Template Groove").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::NewFromTemplate("templates/Groove.suhina".into())));
        harness.get_by_label("Recent Night").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::Open("songs/Night.suhina".into())));
    }

    #[test]
    fn a_typed_path_is_opened_and_escape_closes() {
        let mut harness = dialog(Lists::default());
        harness.run();
        harness.get_by_label("Project path").click();
        harness.run();
        harness.get_by_label("Project path").type_text("\"C:/songs/a.als\"");
        harness.run();
        harness.get_by_label("Open the project at the path").click();
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::Open("C:/songs/a.als".into())));
        harness.state_mut().1 = None;
        harness.key_press(Key::Escape);
        harness.run();
        assert_eq!(harness.state().1, Some(Choice::Close));
    }

    #[test]
    fn the_save_prompt_makes_the_path_from_name_and_folder() {
        let prompt =
            SavePrompt { template: false, name: String::new(), folder: "songs".into(), focus: true, collect: false };
        let mut harness = Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
            |ui, (prompt, chosen): &mut (SavePrompt, Option<SaveChoice>)| {
                let area = ui.max_rect();
                if let Some(choice) = show_save(ui, area, prompt) {
                    *chosen = Some(choice);
                }
            },
            (prompt, None),
        );
        harness.run();
        // Without a name there is nothing to save.
        harness.get_by_label("Save").click();
        harness.run();
        assert_eq!(harness.state().1, None);
        harness.get_by_label("Name").click();
        harness.run();
        harness.get_by_label("Name").type_text("Night drive");
        harness.run();
        harness.get_by_label("Save").click();
        harness.run();
        assert_eq!(harness.state().1, Some(SaveChoice::Save(Path::new("songs").join("Night drive.suhina"))));
        harness.get_by_label("Cancel saving").click();
        harness.run();
        assert_eq!(harness.state().1, Some(SaveChoice::Cancel));
    }

    #[test]
    fn the_offer_of_a_backup_opens_shows_or_ignores_it() {
        for (label, choice) in [
            ("Open the backup", RecoveryChoice::Open),
            ("Show the backup's folder", RecoveryChoice::ShowFolder),
            ("Ignore the backup", RecoveryChoice::Ignore),
            ("Close Backup found", RecoveryChoice::Ignore),
        ] {
            let mut harness = Harness::builder().with_size(egui::vec2(900.0, 600.0)).build_ui_state(
                |ui, chosen: &mut Option<RecoveryChoice>| {
                    let area = ui.max_rect();
                    if let Some(choice) = show_recovery(ui, area, "Song", "2026-10-15 14:05", true) {
                        *chosen = Some(choice);
                    }
                },
                None,
            );
            harness.run();
            harness.get_by_label(label).click();
            harness.run();
            assert_eq!(*harness.state(), Some(choice), "{label}");
        }
    }
}
