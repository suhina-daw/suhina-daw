// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Missing files (`docs/spec/missing-files.md`): the bar that says a project's audio files are
//! missing, and the panel that finds them and relinks them. A file found by hand (Locate…, a
//! file dropped or a path pasted) tells where the others went: its folder, the folder it moved
//! into, and that folder's subfolders are looked in for them.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};

use egui::{Align2, Color32, FontId, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use suhina_library::find::{Found, Walked, rank_candidates, walk_for_names};
use suhina_session::{ProjectId, Projects};

use crate::kit::theme::{self, PAD};
use crate::kit::{table, widgets};

/// One row of the table.
const ROW: f32 = 24.0;
/// The bar with the title, and the one at the bottom with the totals, Close and Apply.
const TOP: f32 = 40.0;
const STATUS: f32 = 44.0;
/// The line with the folder to search, and the one with the filter.
const TOOL: f32 = 26.0;
/// The selected file's panel, and the narrowest the table is kept beside it: a narrower dialog
/// drops the panel.
const DETAIL: f32 = 290.0;
const TABLE_MIN: f32 = 440.0;
/// The widest the dialog is.
const WIDEST: f32 = 1180.0;

/// The columns, by index: the state's mark, the file's name, what uses it, its state, the file
/// it will be linked to, and Locate….
const COLUMNS: [&str; 6] = ["", "File name", "Used by", "Status", "Replacement", ""];
const WIDTHS: [f32; 6] = [22.0, 250.0, 120.0, 96.0, 0.0, 84.0];
const MIN_WIDTHS: [f32; 6] = [22.0, 90.0, 60.0, 60.0, 120.0, 84.0];
/// The column that takes the width the others leave.
const FILL: usize = 4;
/// The widest a double-click fits a column to.
const FIT_MAX: f32 = 520.0;

/// One missing file and what may replace it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// As the project's clips name it.
    pub file: String,
    pub last_path: PathBuf,
    pub clips: usize,
    pub samplers: usize,
    /// Files with its name, best first.
    pub candidates: Vec<PathBuf>,
    /// The one to link to, if any.
    pub chosen: Option<PathBuf>,
    /// The user chose it, so searches leave it.
    pub by_hand: bool,
}

/// How far a row is from being relinked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Missing,
    Found,
    /// Found, with other files of its name to choose from.
    Several(usize),
    /// Chosen by hand.
    Chosen,
}

impl State {
    fn word(self) -> String {
        match self {
            Self::Missing => "Missing".into(),
            Self::Found => "Found".into(),
            Self::Several(n) => format!("{n} found"),
            Self::Chosen => "Chosen".into(),
        }
    }

    fn ink(self, inks: &Inks) -> Color32 {
        match self {
            Self::Missing => inks.error,
            Self::Found => inks.ok,
            Self::Several(_) => inks.warning,
            Self::Chosen => inks.accent,
        }
    }
}

impl Row {
    fn name(&self) -> String {
        Path::new(&self.file)
            .file_name()
            .or_else(|| self.last_path.file_name())
            .map_or_else(|| self.file.clone(), |n| n.to_string_lossy().into_owned())
    }

    fn state(&self) -> State {
        match (&self.chosen, self.by_hand, self.candidates.len()) {
            (None, _, _) => State::Missing,
            (Some(_), true, _) => State::Chosen,
            (Some(_), false, n) if n > 1 => State::Several(n),
            (Some(_), false, _) => State::Found,
        }
    }

    /// What plays it: "38 clips", "1 sampler", "2 clips, 1 sampler".
    fn uses(&self) -> String {
        let count = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
        let mut parts = Vec::new();
        if self.clips > 0 {
            parts.push(count(self.clips, "clip"));
        }
        if self.samplers > 0 {
            parts.push(count(self.samplers, "sampler"));
        }
        if parts.is_empty() { "Nothing".into() } else { parts.join(", ") }
    }

    /// Takes in candidates from a search, unless the user chose one by hand.
    fn offer(&mut self, mut paths: Vec<PathBuf>) {
        for path in paths.drain(..) {
            if !self.candidates.contains(&path) {
                self.candidates.push(path);
            }
        }
        rank_candidates(&self.last_path, &mut self.candidates);
        if !self.by_hand {
            self.chosen = self.candidates.first().cloned();
        }
    }

    /// Links it to `path`, chosen by hand.
    fn choose(&mut self, path: PathBuf) {
        if !self.candidates.contains(&path) {
            self.candidates.insert(0, path.clone());
        }
        self.chosen = Some(path);
        self.by_hand = true;
    }
}

/// A folder walked on another thread.
struct Walk {
    stop: Arc<AtomicBool>,
    walked: Arc<AtomicUsize>,
    done: Receiver<Result<Walked, String>>,
    folder: PathBuf,
}

/// Which rows the table shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Missing,
    Found,
}

/// What the window does after the panel.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Relink these files (the clips' name, the new file).
    Apply(Vec<(String, PathBuf)>),
    /// Show the system's file dialog for the selected file (Locate…).
    PickFile,
    /// Show the system's folder dialog for the folder to search (Browse…).
    PickFolder,
    /// Show this file in the system's file manager.
    Reveal(PathBuf),
}

/// The missing files of the active project, the bar and the panel.
#[derive(Default)]
pub struct MissingFiles {
    /// The project and revision the rows were made for.
    checked: Option<(ProjectId, u64)>,
    /// What the project's audio files were when they were last looked for on disk
    /// ([`audio_files_key`]): an edit that leaves them as they were (a solo, a mute) does not
    /// look again, which took a large project 300 ms or more each time.
    files_key: Option<u64>,
    /// The project's own folder, walked with the browser's index.
    project_folder: Option<PathBuf>,
    /// Search again was clicked: the next [`MissingFiles::sync`] searches the index and the
    /// project's folder again.
    search_again: bool,
    pub rows: Vec<Row>,
    /// The bar was dismissed with Later for this project.
    pub bar_hidden: bool,
    pub open: bool,
    /// The folder typed into the panel's search field.
    pub folder: String,
    walk: Option<Walk>,
    pub note: Option<String>,
    /// The row picked (by its file), shown in the panel at the right.
    pub selected: Option<String>,
    pub filter: Filter,
    /// The words typed into the filter field.
    pub search: String,
    /// The columns' widths, as dragged.
    widths: [f32; 6],
    /// The rows drawn this frame, by file: where a file is dropped.
    row_rects: Vec<(Rect, String)>,
}

impl MissingFiles {
    /// Lists the active project's missing files again when the project or its revision
    /// changed, and searches the browser's index for them at once (a list in memory). A new
    /// project shows the bar again.
    pub fn sync(&mut self, projects: &mut Projects, revision: u64) {
        let Some(active) = projects.active() else {
            self.rows.clear();
            self.checked = None;
            self.open = false;
            return;
        };
        let again = std::mem::take(&mut self.search_again);
        if self.checked == Some((active, revision)) && !again {
            return;
        }
        let new_project = self.checked.is_none_or(|(id, _)| id != active);
        if new_project {
            self.bar_hidden = false;
            self.open = false;
            self.rows.clear();
            self.note = None;
            self.selected = None;
        }
        self.checked = Some((active, revision));
        let Some(session) = projects.get(active) else { return };
        let key = audio_files_key(session.project());
        if !new_project && !again && self.files_key == Some(key) {
            return;
        }
        self.files_key = Some(key);
        let missing = session.missing_files();
        self.project_folder = session.audio_folder().map(Path::to_path_buf);
        // Keep what was found or chosen for files still missing.
        let old = std::mem::take(&mut self.rows);
        self.rows = missing
            .into_iter()
            .map(|m| {
                let kept = old.iter().find(|r| r.file == m.file).cloned();
                let mut row = kept.unwrap_or(Row {
                    file: m.file,
                    last_path: m.last_path,
                    clips: 0,
                    samplers: 0,
                    candidates: Vec::new(),
                    chosen: None,
                    by_hand: false,
                });
                (row.clips, row.samplers) = (m.clips, m.samplers);
                row
            })
            .collect();
        if self.rows.is_empty() {
            self.open = false;
            return;
        }
        let names = self.names();
        let found = projects.file_library_mut().find_by_names(&names);
        self.take(found);
        if !self.selected.as_ref().is_some_and(|s| self.rows.iter().any(|r| &r.file == s)) {
            self.selected = self.rows.iter().find(|r| r.chosen.is_none()).or(self.rows.first()).map(|r| r.file.clone());
        }
        // The project's own folder is walked too, on a thread of its own.
        if (new_project || again)
            && let Some(folder) = self.project_folder.clone().filter(|f| f.is_dir())
            && self.walk.is_none()
        {
            self.start_walk(folder);
        }
    }

    /// The names of the files still to look for: those not chosen by hand.
    fn names(&self) -> Vec<String> {
        self.rows.iter().filter(|r| !r.by_hand).map(Row::name).collect()
    }

    fn take(&mut self, found: Found) {
        for (name, paths) in found {
            for row in self.rows.iter_mut().filter(|r| r.name().eq_ignore_ascii_case(&name)) {
                row.offer(paths.clone());
            }
        }
    }

    /// Walks `folder` and its subfolders on a thread of its own for the files still missing.
    fn start_walk(&mut self, folder: PathBuf) {
        let names = self.names();
        if names.is_empty() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let walked = Arc::new(AtomicUsize::new(0));
        let (send, done) = channel();
        let (s, w, f) = (Arc::clone(&stop), Arc::clone(&walked), folder.clone());
        let started = std::thread::Builder::new().name("suhina-find-files".into()).spawn(move || {
            let _ = send.send(walk_for_names(&f, &names, &s, &w));
        });
        if started.is_ok() {
            self.walk = Some(Walk { stop, walked, done, folder });
        }
    }

    /// Takes in a finished walk. Returns whether one is still going.
    fn poll(&mut self) -> bool {
        let Some(walk) = &self.walk else { return false };
        match walk.done.try_recv() {
            Ok(result) => {
                self.finish_walk(Some(result));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.finish_walk(None);
                false
            }
        }
    }

    /// Takes in what a walk found (`None` when its thread ended without a word).
    fn finish_walk(&mut self, result: Option<Result<Walked, String>>) {
        let Some(walk) = self.walk.take() else { return };
        match result {
            Some(Ok(walked)) => {
                let folder = walk.folder.display().to_string();
                let before = self.found_count();
                self.take(walked.found);
                let more = self.found_count().saturating_sub(before);
                self.note = Some(match walked.note {
                    Some(note) => format!("{folder}: {note}"),
                    None => format!("{folder}: {more} found"),
                });
            }
            Some(Err(e)) => self.note = Some(e),
            None => {}
        }
    }

    /// Waits for the walk running to end and takes in what it found (tests: the walk's own
    /// channel is the signal).
    #[cfg(test)]
    fn wait_for_walk(&mut self) {
        let result = self.walk.as_ref().and_then(|walk| walk.done.recv().ok());
        self.finish_walk(result);
    }

    fn found_count(&self) -> usize {
        self.rows.iter().filter(|r| r.chosen.is_some()).count()
    }

    /// The files that have a candidate, for relinking.
    fn chosen(&self) -> Vec<(String, PathBuf)> {
        self.rows.iter().filter_map(|r| Some((r.file.clone(), r.chosen.clone()?))).collect()
    }

    /// Links `file`'s row to `path`, chosen by hand, then looks for the other missing files
    /// where it was: beside it, where its old folder moved to, and (on a thread) in its folder's
    /// subfolders. Says how many more it found.
    pub fn locate(&mut self, file: &str, path: PathBuf) {
        let Some(index) = self.rows.iter().position(|r| r.file == file) else { return };
        let missing = |rows: &[Row]| rows.iter().enumerate().filter(|(i, r)| *i != index && r.chosen.is_none()).count();
        let missing_before = missing(&self.rows);
        let Some(row) = self.rows.get_mut(index) else { return };
        let last = row.last_path.clone();
        row.choose(path.clone());
        self.selected = Some(file.to_string());
        self.look_beside(index, &last, &path);
        let more = missing_before.saturating_sub(missing(&self.rows));
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        self.note = Some(match more {
            0 => format!("Located {name}"),
            1 => format!("Located {name}, found 1 more beside it"),
            n => format!("Located {name}, found {n} more beside it"),
        });
        if self.walk.is_none()
            && self.rows.iter().any(|r| r.chosen.is_none())
            && let Some(folder) = path.parent().filter(|f| f.is_dir())
        {
            self.start_walk(folder.to_path_buf());
        }
    }

    /// Offers the other rows the files of their name in `path`'s folder and where the folder
    /// `last` was in moved to (`last` and `path` less the folders and name they end in alike):
    /// a sample library moved to another drive keeps its subfolders.
    fn look_beside(&mut self, located: usize, last: &Path, path: &Path) {
        let folder = path.parent();
        let moved = moved_folder(last, path);
        for (index, row) in self.rows.iter_mut().enumerate() {
            if index == located || row.by_hand {
                continue;
            }
            let mut paths = Vec::new();
            if let Some(folder) = folder {
                let beside = folder.join(row.name());
                if beside.is_file() {
                    paths.push(beside);
                }
            }
            if let Some((old, new)) = &moved
                && let Ok(rest) = row.last_path.strip_prefix(old)
            {
                let there = new.join(rest);
                if there.is_file() && !paths.contains(&there) {
                    paths.push(there);
                }
            }
            if !paths.is_empty() {
                row.offer(paths);
            }
        }
    }

    /// A path dropped onto the panel or pasted into it: a folder is searched; a file is the
    /// one `target` names (the row it was dropped on, or the selected one), or else the
    /// missing file of its name.
    pub fn take_path(&mut self, path: PathBuf, target: Option<String>) {
        if path.is_dir() {
            self.folder = path.display().to_string();
            self.search_folder(path);
            return;
        }
        if !path.is_file() {
            self.note = Some(format!("No file at {}", path.display()));
            return;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let by_name = || self.rows.iter().find(|r| r.name().eq_ignore_ascii_case(&name)).map(|r| r.file.clone());
        match target.or_else(by_name) {
            Some(file) => self.locate(&file, path),
            None => self.note = Some(format!("{name} is not one of the missing files")),
        }
    }

    /// Searches `folder` and its subfolders for the files still missing.
    pub fn search_folder(&mut self, folder: PathBuf) {
        match &self.walk {
            Some(_) => self.note = Some("A search is running: stop it first".into()),
            None if !folder.is_dir() => self.note = Some(format!("{} is not a folder", folder.display())),
            None => {
                self.note = None;
                self.start_walk(folder);
            }
        }
    }

    /// Where the file dialog of Locate… opens: the nearest folder of the selected file's old
    /// path that is still there.
    pub fn pick_start(&self) -> Option<PathBuf> {
        let row = self.selected_row()?;
        row.last_path.ancestors().skip(1).find(|p| p.is_dir()).map(Path::to_path_buf)
    }

    /// The file the system's dialog chose for the selected row.
    pub fn picked_file(&mut self, path: PathBuf) {
        let target = self.selected.clone();
        self.take_path(path, target);
    }

    fn selected_row(&self) -> Option<&Row> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().find(|r| &r.file == selected)
    }

    /// The rows the filter and the search words show, by index.
    fn shown(&self) -> Vec<usize> {
        let words: Vec<String> = self.search.split_whitespace().map(str::to_lowercase).collect();
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| match self.filter {
                Filter::All => true,
                Filter::Missing => row.chosen.is_none(),
                Filter::Found => row.chosen.is_some(),
            })
            .filter(|(_, row)| {
                let text = format!("{} {}", row.file, row.last_path.display()).to_lowercase();
                words.iter().all(|w| text.contains(w))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Draws the bar that says files are missing in `rect`, while it should show.
    pub fn bar(&mut self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, crate::kit::style_class::surface_or(crate::kit::element::app_fill("panel")));
        let count = self.rows.len();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(PAD, 6.0))), |ui| {
            ui.horizontal_centered(|ui| {
                let text = format!("{count} audio file{} missing", if count == 1 { " is" } else { "s are" });
                ui.label(egui::RichText::new(text).color(crate::kit::element::name_ink()));
                if ui.button("Find…").clicked() {
                    self.open = true;
                }
                if ui.button("Later").clicked() {
                    self.bar_hidden = true;
                }
            });
        });
    }

    /// Whether the bar shows.
    pub fn bar_shows(&self) -> bool {
        !self.rows.is_empty() && !self.bar_hidden && !self.open
    }

    /// Draws the panel over `area` while it is open.
    pub fn panel(&mut self, ui: &Ui, area: Rect, project: &str) -> Option<Outcome> {
        if !self.open {
            return None;
        }
        if self.poll() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        // The panel is a dialog: the theme may style it, and its panel does not take the window's colour.
        let dialog = crate::kit::element::DIALOG.enter();
        crate::kit::floating::area(ui.ctx(), Id::unique("missing-files"), crate::kit::floating::Kind::Own)
            .order(egui::Order::Foreground)
            .fixed_pos(area.min)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(area.size());
                self.draw(ui, &dialog, area, project)
            })
            .inner
    }

    fn draw(
        &mut self,
        ui: &mut Ui,
        dialog: &crate::kit::element::Entered,
        area: Rect,
        project: &str,
    ) -> Option<Outcome> {
        let behind = ui.interact(area, Id::unique("missing-files-behind"), Sense::click_and_drag());
        let _swallowed = behind.clicked();
        let painter = ui.painter_at(area);
        painter.rect_filled(
            area,
            0.0,
            crate::kit::style_class::opaque_surface_or(crate::kit::element::app_fill("window")).gamma_multiply(0.6),
        );
        let wanted = TOP + 2.0 * TOOL + 3.0 * PAD * 1.5 + ROW + 4.0 + self.rows.len() as f32 * ROW + STATUS + 24.0;
        let wanted = Vec2::new(WIDEST, wanted.max(460.0));
        let size = crate::kit::floating::dialog_size(dialog, area, wanted);
        let rect = Rect::from_center_size(area.center(), size);
        let fill = crate::kit::style_class::own_background_or(crate::kit::element::app_fill("panel"));
        // What the dialog holds goes in its content box, inside its border and padding (#1336).
        let inner = crate::kit::floating::dialog(&painter, dialog, rect, fill);
        let painter = painter.with_clip_rect(inner);
        let inks = Inks::read();
        let mut outcome = None;

        let top = Rect::from_min_size(inner.min, Vec2::new(inner.width(), TOP));
        let status = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - STATUS), inner.max);
        let body = Rect::from_min_max(Pos2::new(inner.left(), top.bottom()), Pos2::new(inner.right(), status.top()));
        let detail = (body.width() >= DETAIL + TABLE_MIN)
            .then(|| Rect::from_min_max(Pos2::new(body.right() - DETAIL, body.top()), body.max));
        let middle = Rect::from_min_max(body.min, Pos2::new(detail.map_or(body.right(), |d| d.left()), body.bottom()));
        let line = theme::hairline(inks.line);
        painter.hline(status.x_range(), status.top() + 0.5, line);
        if let Some(detail) = detail {
            painter.vline(detail.left() + 0.5, detail.y_range(), line);
        }

        self.top_bar(ui, &painter, top, project, &inks);
        let table_rect = self.tools(ui, &painter, middle.shrink(PAD * 1.5), &inks, &mut outcome);
        self.table(ui, &painter, table_rect, &inks, &mut outcome);
        if let Some(detail) = detail {
            self.detail(ui, &painter, detail.shrink2(Vec2::new(PAD * 2.5, PAD * 2.0)), &inks, &mut outcome);
        }
        self.status_bar(ui, &painter, status.shrink2(Vec2::new(PAD * 2.0, 0.0)), &inks, &mut outcome);

        self.take_drops(ui, detail);
        self.take_paste(ui);
        self.arrow_keys(ui);
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if ui.input(|i| i.key_pressed(Key::Escape)) && !typing && !egui::Popup::is_any_open(ui.ctx()) {
            self.close();
        }
        if matches!(outcome, Some(Outcome::Apply(_))) {
            self.close();
        }
        outcome
    }

    /// The title and how many files; Search again on the right.
    fn top_bar(&mut self, ui: &Ui, painter: &egui::Painter, rect: Rect, project: &str, inks: &Inks) {
        let title_row = crate::kit::element::DIALOG_TITLE.enter();
        painter.rect_filled(rect, 0.0, title_row.background());
        let rect = rect.shrink2(Vec2::new(PAD * 2.0, 0.0));
        let title_font = theme::ui_font(theme::TEXT + 3.0);
        let title = "Missing files";
        let title_width = widgets::text_width(ui, title, title_font.clone());
        widgets::text_in(painter, rect, Align2::LEFT_CENTER, title, title_font, crate::kit::element::title_ink());
        let small = theme::ui_font(theme::TEXT_SMALL);
        let count = self.rows.len();
        let under = format!("{project} · {count} file{}", if count == 1 { "" } else { "s" });
        let text = "Search again";
        let width = widgets::text_width(ui, text, small.clone()) + 28.0;
        let button =
            Rect::from_min_size(Pos2::new(rect.right() - width, rect.center().y - 12.0), Vec2::new(width, 24.0));
        let note = Rect::from_min_max(
            Pos2::new(rect.left() + title_width + 14.0, rect.top()),
            Pos2::new(button.left() - 12.0, rect.bottom()),
        );
        widgets::text_in(painter, note, Align2::LEFT_CENTER, &under, small, inks.secondary);
        let label = "Search the browser's folders and the project's folder again";
        let id = Id::unique("missing-search-again");
        if widgets::button_with_id(ui, painter, button, id, text, label, false).clicked() {
            self.search_again = true;
        }
    }

    /// The folder to search, with Browse… and Search (Stop while one runs), and under it the
    /// filter field and the All, Missing and Found filters. Returns the room left for the table.
    fn tools(
        &mut self,
        ui: &mut Ui,
        painter: &egui::Painter,
        rect: Rect,
        inks: &Inks,
        outcome: &mut Option<Outcome>,
    ) -> Rect {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let line_at = |y: f32| Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), TOOL));
        // The folder to search.
        let first = line_at(rect.top());
        let label_width = widgets::text_width(ui, "Search a folder", small.clone()) + 10.0;
        widgets::text_in(painter, first, Align2::LEFT_CENTER, "Search a folder", small.clone(), inks.secondary);
        let buttons = 180.0;
        let field = Rect::from_min_max(
            Pos2::new(first.left() + label_width, first.top() + 1.0),
            Pos2::new(first.right() - buttons - 8.0, first.bottom() - 1.0),
        );
        let edit = ui.put(
            field,
            egui::TextEdit::singleline(&mut self.folder)
                .frame(widgets::input_frame())
                .hint_text("A folder, or drop one here")
                .font(small.clone()),
        );
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Folder to search"));
        widgets::input_line(ui, &edit);
        let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let browse = Rect::from_min_size(Pos2::new(field.right() + 8.0, first.top()), Vec2::new(86.0, TOOL));
        let id = Id::unique("missing-browse");
        if widgets::button_with_id(ui, painter, browse, id, "Browse…", "Choose a folder to search", false).clicked() {
            *outcome = Some(Outcome::PickFolder);
        }
        let search = Rect::from_min_max(Pos2::new(browse.right() + 6.0, first.top()), first.max);
        match &self.walk {
            Some(walk) => {
                let id = Id::unique("missing-stop");
                if widgets::button_with_id(ui, painter, search, id, "Stop", "Stop searching", false).clicked() {
                    walk.stop.store(true, Ordering::Relaxed);
                }
            }
            None => {
                let id = Id::unique("missing-search");
                let clicked =
                    widgets::button_with_id(ui, painter, search, id, "Search", "Search the folder", false).clicked();
                if (clicked || entered) && !self.folder.trim().is_empty() {
                    self.search_folder(PathBuf::from(self.folder.trim()));
                }
            }
        }

        // The filter field and the filters, each with its count.
        let second = line_at(first.bottom() + PAD * 1.5);
        let missing = self.rows.iter().filter(|r| r.chosen.is_none()).count();
        let filters = [
            (Filter::All, "All", self.rows.len()),
            (Filter::Missing, "Missing", missing),
            (Filter::Found, "Found", self.rows.len() - missing),
        ];
        let texts: Vec<String> = filters.iter().map(|(_, name, n)| format!("{name} {n}")).collect();
        let widths: Vec<f32> = texts.iter().map(|t| widgets::text_width(ui, t, small.clone()) + 22.0).collect();
        let segments: f32 = widths.iter().sum();
        let search_field = Rect::from_min_max(
            Pos2::new(second.left(), second.top() + 1.0),
            Pos2::new((second.right() - segments - 12.0).max(second.left() + 80.0), second.bottom() - 1.0),
        );
        let edit = ui.put(
            search_field,
            egui::TextEdit::singleline(&mut self.search)
                .frame(widgets::input_frame())
                .hint_text("Filter files")
                .font(small.clone()),
        );
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find a missing file"));
        widgets::input_line(ui, &edit);
        let mut x = second.right() - segments;
        for (((filter, name, count), text), width) in filters.iter().zip(&texts).zip(&widths) {
            let segment = Rect::from_min_size(Pos2::new(x, second.top()), Vec2::new(*width, TOOL));
            x += width;
            let label = format!("Show {} ({count})", name.to_lowercase());
            let response = widgets::button_area(ui, segment, Id::unique(("missing-filter", name)), label);
            let on = self.filter == *filter;
            let hovered = widgets::hover(ui, &response);
            if on {
                painter.rect_filled(segment, 3.0, inks.picked);
            } else if hovered > 0.0 {
                painter.rect_filled(segment, 3.0, inks.hovered.gamma_multiply(hovered));
            }
            painter.rect_stroke(segment, 3.0, theme::hairline(inks.strong_line), egui::StrokeKind::Inside);
            let ink = if on { inks.primary } else { inks.secondary };
            widgets::text_in(painter, segment, Align2::CENTER_CENTER, text, small.clone(), ink);
            if response.clicked() {
                self.filter = *filter;
            }
        }
        Rect::from_min_max(Pos2::new(rect.left(), second.bottom() + PAD * 1.5), rect.max)
    }

    /// The columns' left and right edges across `area`: the replacement takes what the others
    /// leave, at least its narrowest.
    fn spans(&self, area: Rect) -> [(f32, f32); 6] {
        let mut widths = self.column_widths();
        let others: f32 = widths.iter().enumerate().filter(|(i, _)| *i != FILL).map(|(_, w)| w).sum();
        widths[FILL] = (area.width() - others).max(MIN_WIDTHS[FILL]);
        let mut x = area.left();
        let mut spans = [(0.0, 0.0); 6];
        for (span, width) in spans.iter_mut().zip(widths) {
            *span = (x, x + width);
            x += width;
        }
        spans
    }

    fn column_widths(&self) -> [f32; 6] {
        let mut widths = WIDTHS;
        for ((width, kept), least) in widths.iter_mut().zip(self.widths).zip(MIN_WIDTHS) {
            if kept > 0.0 {
                *width = kept.max(least);
            }
        }
        widths
    }

    /// The width that fits a column's heading and every row's text in it.
    fn fit_width(&self, ui: &Ui, column: usize) -> f32 {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let mut widest = widgets::text_width(ui, COLUMNS[column], small.clone()) + 16.0;
        for row in &self.rows {
            let text = match column {
                1 => row.name(),
                2 => row.uses(),
                _ => row.state().word(),
            };
            widest = widest.max(widgets::text_width(ui, &text, small.clone()) + 12.0);
        }
        widest.clamp(MIN_WIDTHS[column], FIT_MAX)
    }

    /// The headings, the rows (scrolled both ways under headings that stay) and what a click,
    /// a right-click or Locate… on a row asks for.
    fn table(&mut self, ui: &mut Ui, painter: &egui::Painter, rect: Rect, inks: &Inks, outcome: &mut Option<Outcome>) {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let shown = self.shown();
        let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW + 4.0));
        let body = Rect::from_min_max(Pos2::new(rect.left(), strip.bottom() + 1.0), rect.max);
        let total = {
            let spans = self.spans(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 1.0)));
            spans[5].1 - rect.left()
        };
        self.row_rects.clear();
        if shown.is_empty() {
            let text = if self.rows.is_empty() { "No files are missing" } else { "No file matches the filter" };
            widgets::text_in(painter, body.shrink(PAD * 2.0), Align2::CENTER_TOP, text, small.clone(), inks.hint);
        }
        let mut asked = Asked::default();
        let mut drawn: Vec<(Rect, String)> = Vec::new();
        let hovering_files = ui.input(|i| !i.raw.hovered_files.is_empty());
        let across = ui
            .scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::both().id_salt("missing-files-rows").auto_shrink([false, false]).show_rows(
                    ui,
                    ROW,
                    shown.len(),
                    |ui, range| {
                        for &index in shown.get(range).unwrap_or_default() {
                            let Some(row) = self.rows.get(index) else { continue };
                            let (rect, response) = ui.allocate_exact_size(Vec2::new(total, ROW), Sense::click());
                            let spans = self.spans(Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW)));
                            let selected = self.selected.as_deref() == Some(row.file.as_str());
                            let painter = ui.painter_at(rect);
                            let view = RowView { rect, spans, selected, hovering_files, inks, font: &small };
                            row_view(ui, &painter, &response, row, &view, &mut asked);
                            drawn.push((rect.intersect(body), row.file.clone()));
                        }
                    },
                )
            })
            .inner
            .state
            .unclamped_offset()
            .x;
        self.row_rects = drawn;
        // The headings follow the rows' sideways scroll.
        let header =
            Rect::from_min_size(Pos2::new(strip.left() - across, strip.top()), Vec2::new(total, strip.height()));
        let header_painter = painter.with_clip_rect(strip.intersect(painter.clip_rect()));
        self.headings(ui, &header_painter, header, strip, inks);

        if let Some(file) = asked.select {
            self.selected = Some(file);
        }
        if let Some(text) = asked.copy {
            ui.ctx().copy_text(text);
        }
        if let Some(file) = asked.leave
            && let Some(row) = self.rows.iter_mut().find(|r| r.file == file)
        {
            row.chosen = None;
            row.by_hand = true;
        }
        if let Some(file) = asked.locate {
            self.selected = Some(file);
            *outcome = Some(Outcome::PickFile);
        }
        if let Some(path) = asked.reveal {
            *outcome = Some(Outcome::Reveal(path));
        }
    }

    /// The headings across `header`, seen through `strip`, and the edges dragged to resize the
    /// name's, the uses' and the state's columns.
    fn headings(&mut self, ui: &Ui, painter: &egui::Painter, header: Rect, strip: Rect, inks: &Inks) {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let spans = self.spans(header);
        painter.hline(strip.x_range(), header.bottom() - 0.5, theme::hairline(inks.line));
        for (name, (left, right)) in COLUMNS.iter().zip(spans) {
            let cell = Rect::from_min_max(Pos2::new(left + 4.0, header.top()), Pos2::new(right - 4.0, header.bottom()));
            widgets::text_in(painter, cell, Align2::LEFT_CENTER, name, small.clone(), inks.secondary);
        }
        for column in 1..FILL {
            let (_, right) = spans[column];
            let id = Id::unique(("missing-edge", column));
            let edge =
                table::resize_edge(ui, painter, header, strip, right, id, COLUMNS[column], (inks.line, inks.accent));
            let mut widths = self.column_widths();
            if edge.fit {
                widths[column] = self.fit_width(ui, column);
            } else if edge.delta != 0.0 {
                widths[column] = (widths[column] + edge.delta).clamp(MIN_WIDTHS[column], FIT_MAX * 2.0);
            } else {
                continue;
            }
            self.widths = widths;
        }
    }

    /// The selected file: its name, state and uses, where it was (with Copy file name and Copy
    /// path), what it will be linked to, Locate file…, and the other files of its name to choose.
    fn detail(&mut self, ui: &Ui, painter: &egui::Painter, rect: Rect, inks: &Inks, outcome: &mut Option<Outcome>) {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let Some(row) = self.selected_row().cloned() else {
            let at = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 20.0));
            widgets::text_in(painter, at, Align2::LEFT_CENTER, "Pick a file to see it here", small, inks.hint);
            return;
        };
        let rule = |y: f32| painter.hline(rect.x_range(), y, theme::hairline(inks.line));
        let line_at = |y: f32, h: f32| Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), h));
        let mut y = rect.top();
        heading(painter, line_at(y, 18.0), "Selected file", inks);
        y += 18.0;
        let name = row.name();
        y = wrapped(
            painter,
            rect.left(),
            y + 4.0,
            rect.width(),
            &name,
            theme::ui_font(theme::TEXT + 4.0),
            inks.primary,
        );
        y += 6.0;
        let state = row.state();
        let tag_right = tag(ui, painter, Pos2::new(rect.left(), y), &state.word(), state.ink(inks));
        let uses = Rect::from_min_max(Pos2::new(tag_right + 10.0, y), Pos2::new(rect.right(), y + 20.0));
        widgets::text_in(
            painter,
            uses,
            Align2::LEFT_CENTER,
            &format!("Used by {}", row.uses()),
            small.clone(),
            inks.secondary,
        );
        y += 30.0;
        rule(y);
        y += 10.0;

        widgets::text_in(
            painter,
            line_at(y, 18.0),
            Align2::LEFT_CENTER,
            "Original location",
            small.clone(),
            inks.primary,
        );
        let last = row.last_path.display().to_string();
        y = wrapped(painter, rect.left(), y + 20.0, rect.width(), &last, small.clone(), inks.secondary) + 6.0;
        // Copy file name, for a search tool; Copy path beside it.
        let half = (rect.width() - 6.0) / 2.0;
        let copy_name = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(half, 24.0));
        let copy_path = Rect::from_min_size(Pos2::new(copy_name.right() + 6.0, y), Vec2::new(half, 24.0));
        let label = format!("Copy the name of {name}");
        if widgets::button_with_id(
            ui,
            painter,
            copy_name,
            Id::unique("missing-copy-name"),
            "Copy file name",
            &label,
            false,
        )
        .clicked()
        {
            ui.ctx().copy_text(name.clone());
            self.note = Some(format!("Copied {name}"));
        }
        let label = format!("Copy the original path of {name}");
        if widgets::button_with_id(ui, painter, copy_path, Id::unique("missing-copy-path"), "Copy path", &label, false)
            .clicked()
        {
            ui.ctx().copy_text(last.clone());
            self.note = Some(format!("Copied {last}"));
        }
        y += 34.0;
        rule(y);
        y += 10.0;

        widgets::text_in(painter, line_at(y, 18.0), Align2::LEFT_CENTER, "Replacement", small.clone(), inks.primary);
        y += 20.0;
        match &row.chosen {
            Some(chosen) => {
                let text = chosen.display().to_string();
                y = wrapped(painter, rect.left(), y, rect.width(), &text, small.clone(), inks.secondary) + 4.0;
                let id = Id::unique("missing-reveal");
                let label = format!("Show {} in its folder", name);
                if link(ui, painter, line_at(y, 18.0), id, show_in_folder_label(), &label, inks) {
                    *outcome = Some(Outcome::Reveal(chosen.clone()));
                }
                y += 24.0;
            }
            None => {
                widgets::text_in(
                    painter,
                    line_at(y, 18.0),
                    Align2::LEFT_CENTER,
                    "Not found yet",
                    small.clone(),
                    inks.hint,
                );
                y += 22.0;
            }
        }
        let locate = line_at(y, 28.0);
        let label = format!("Locate the selected file, {name}");
        if widgets::button_with_id(
            ui,
            painter,
            locate,
            Id::unique("missing-locate-selected"),
            "Locate file…",
            &label,
            false,
        )
        .clicked()
        {
            *outcome = Some(Outcome::PickFile);
        }
        y += 34.0;
        let hint = "Or drop the file here, or copy its path and press Ctrl+V.";
        y = wrapped(painter, rect.left(), y, rect.width(), hint, small.clone(), inks.hint) + 10.0;

        // The other files of its name, and Leave it missing.
        if row.candidates.len() > 1 || row.chosen.is_some() {
            rule(y);
            y += 10.0;
            heading(painter, line_at(y, 18.0), "Files with its name", inks);
            y += 20.0;
            let mut choice: Option<Option<PathBuf>> = None;
            let options = row.candidates.iter().map(Some).chain(std::iter::once(None));
            for candidate in options {
                if y + ROW > rect.bottom() {
                    break;
                }
                let line = line_at(y, ROW);
                y += ROW;
                let (text, label) = match candidate {
                    Some(path) => (path.display().to_string(), format!("Choose {} for {name}", path.display())),
                    None => ("Leave it missing".to_string(), format!("Leave {name} missing")),
                };
                let id = Id::unique(("missing-candidate", &row.file, &text));
                let response = widgets::button_area(ui, line, id, label);
                let on = row.chosen.as_ref() == candidate;
                if on {
                    painter.rect_filled(line, 3.0, inks.picked);
                } else {
                    let hovered = widgets::hover(ui, &response);
                    if hovered > 0.0 {
                        painter.rect_filled(line, 3.0, inks.hovered.gamma_multiply(hovered));
                    }
                }
                let ink = if on { inks.primary } else { inks.secondary };
                let cell = line.shrink2(Vec2::new(6.0, 0.0));
                table::path_cell(ui, painter, cell, id.with("cell"), &text, small.clone(), ink);
                if response.clicked() {
                    choice = Some(candidate.cloned());
                }
            }
            if let Some(choice) = choice
                && let Some(row) = self.rows.iter_mut().find(|r| r.file == row.file)
            {
                row.chosen = choice;
                row.by_hand = true;
            }
        }
    }

    /// The totals, or the search running with Stop, or what was last done on the left; Close
    /// and Apply on the right.
    fn status_bar(&mut self, ui: &Ui, painter: &egui::Painter, rect: Rect, inks: &Inks, outcome: &mut Option<Outcome>) {
        let small = theme::ui_font(theme::TEXT_SMALL);
        let chosen = self.chosen();
        let apply_text = format!("Apply {}", chosen.len());
        let apply_width = widgets::text_width(ui, &apply_text, small.clone()) + 36.0;
        let apply = Rect::from_min_size(
            Pos2::new(rect.right() - apply_width, rect.center().y - 14.0),
            Vec2::new(apply_width, 28.0),
        );
        let close = Rect::from_min_size(Pos2::new(apply.left() - 96.0, apply.top()), Vec2::new(88.0, 28.0));
        let label = "Point the clips at the files found, one undo step";
        let id = Id::unique("missing-apply");
        if widgets::button_with_id(ui, painter, apply, id, &apply_text, label, !chosen.is_empty()).clicked()
            && !chosen.is_empty()
        {
            *outcome = Some(Outcome::Apply(chosen.clone()));
        }
        let id = Id::unique("missing-close");
        if widgets::button_with_id(ui, painter, close, id, "Close", "Close the missing files panel", false).clicked() {
            self.close();
        }
        let left = Rect::from_min_max(rect.min, Pos2::new(close.left() - 12.0, rect.bottom()));
        let still = self.rows.len() - chosen.len();
        let (dot, first) = match &self.walk {
            Some(walk) => {
                let n = walk.walked.load(Ordering::Relaxed);
                (inks.accent, format!("Searching {}: {n} files", walk.folder.display()))
            }
            None => {
                let ready = chosen.len();
                let ink = if still == 0 { inks.ok } else { inks.warning };
                (ink, format!("{ready} ready to relink · {still} still missing"))
            }
        };
        painter.circle_filled(Pos2::new(left.left() + 4.0, left.center().y - 7.0), 3.5, dot);
        let upper = Rect::from_min_max(
            Pos2::new(left.left() + 14.0, left.top() + 4.0),
            Pos2::new(left.right(), left.center().y),
        );
        widgets::text_in(painter, upper, Align2::LEFT_CENTER, &first, theme::ui_font(theme::TEXT), inks.primary);
        if let Some(note) = &self.note {
            let lower = Rect::from_min_max(
                Pos2::new(left.left() + 14.0, left.center().y),
                Pos2::new(left.right(), left.bottom() - 4.0),
            );
            let (text, _) = table::middle_cut(painter, note, &small, lower.width());
            widgets::text_in(painter, lower, Align2::LEFT_CENTER, &text, small, inks.secondary);
        }
    }

    /// Files dropped from the system: on a row, that row's file; on the panel at the right, the
    /// selected one's; elsewhere, the missing file of each one's name. A folder is searched.
    fn take_drops(&mut self, ui: &Ui, detail: Option<Rect>) {
        let (dropped, pointer): (Vec<PathBuf>, Option<Pos2>) = ui.input(|i| {
            let paths =
                i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty());
            (paths.collect(), i.pointer.latest_pos())
        });
        if dropped.is_empty() {
            return;
        }
        let on_row = pointer.and_then(|at| self.row_rects.iter().find(|(r, _)| r.contains(at)).map(|(_, f)| f.clone()));
        let on_detail = pointer.is_some_and(|at| detail.is_some_and(|d| d.contains(at)));
        let target = on_row.or_else(|| on_detail.then(|| self.selected.clone()).flatten());
        // One file goes where it was dropped; several each find the file of their name.
        let one = dropped.len() == 1;
        for path in dropped {
            self.take_path(path, if one { target.clone() } else { None });
        }
    }

    /// A path pasted (Ctrl+V) while no field takes it: the selected row's file, or a folder to search.
    fn take_paste(&mut self, ui: &Ui) {
        if ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        let pasted = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Paste(text) => pasted_path(text),
                _ => None,
            })
        });
        if let Some(path) = pasted {
            let target = self.selected.clone();
            self.take_path(path, target);
        }
    }

    /// Up and Down move the selection through the rows shown.
    fn arrow_keys(&mut self, ui: &Ui) {
        if ui.ctx().memory(|m| m.focused().is_some()) || egui::Popup::is_any_open(ui.ctx()) {
            return;
        }
        let (up, down) = ui.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
        if !up && !down {
            return;
        }
        let shown = self.shown();
        let at = shown.iter().position(|&i| self.rows.get(i).map(|r| &r.file) == self.selected.as_ref());
        let next = match (at, up) {
            (Some(at), true) => at.saturating_sub(1),
            (Some(at), false) => (at + 1).min(shown.len().saturating_sub(1)),
            (None, _) => 0,
        };
        if let Some(row) = shown.get(next).and_then(|&i| self.rows.get(i)) {
            self.selected = Some(row.file.clone());
        }
    }

    fn close(&mut self) {
        self.open = false;
        if let Some(walk) = &self.walk {
            walk.stop.store(true, Ordering::Relaxed);
        }
    }
}

/// What a row asked for this frame.
#[derive(Default)]
struct Asked {
    select: Option<String>,
    locate: Option<String>,
    copy: Option<String>,
    leave: Option<String>,
    reveal: Option<PathBuf>,
}

/// What every row of a frame shares, and its own rectangle and columns.
struct RowView<'a> {
    rect: Rect,
    spans: [(f32, f32); 6],
    selected: bool,
    /// Files are dragged over the window: the row under the pointer is marked.
    hovering_files: bool,
    inks: &'a Inks,
    font: &'a FontId,
}

/// One row: its fill, its cells with their whole text on hover, Locate…, its click and its
/// right-click menu.
fn row_view(
    ui: &mut Ui,
    painter: &egui::Painter,
    response: &egui::Response,
    row: &Row,
    view: &RowView<'_>,
    asked: &mut Asked,
) {
    let RowView { rect, spans, selected, inks, .. } = *view;
    let name = row.name();
    let pointed = ui.rect_contains_pointer(rect);
    if selected {
        painter.rect_filled(rect, 0.0, inks.picked);
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height())), 0.0, inks.accent);
    } else if pointed {
        painter.rect_filled(rect, 0.0, inks.hovered);
    }
    if view.hovering_files && pointed {
        painter.rect_stroke(rect.shrink(1.0), 2.0, egui::Stroke::new(1.5, inks.accent), egui::StrokeKind::Inside);
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(inks.line.gamma_multiply(0.5)));
    if response.clicked() || response.secondary_clicked() {
        asked.select = Some(row.file.clone());
    }
    if response.double_clicked() {
        asked.locate = Some(row.file.clone());
    }
    response.context_menu(|ui| {
        if ui.button("Copy file name").clicked() {
            asked.copy = Some(name.clone());
            ui.close();
        }
        if ui.button("Copy original path").clicked() {
            asked.copy = Some(row.last_path.display().to_string());
            ui.close();
        }
        ui.separator();
        if ui.button("Locate…").clicked() {
            asked.locate = Some(row.file.clone());
            ui.close();
        }
        if let Some(chosen) = &row.chosen {
            if ui.button(show_in_folder_label()).clicked() {
                asked.reveal = Some(chosen.clone());
                ui.close();
            }
            if ui.button("Leave it missing").clicked() {
                asked.leave = Some(row.file.clone());
                ui.close();
            }
        }
    });
    let state = row.state();
    let font = view.font.clone();
    let cell = |column: usize| {
        let (left, right) = spans[column];
        Rect::from_min_max(Pos2::new(left + 4.0, rect.top()), Pos2::new(right - 4.0, rect.bottom()))
    };
    let mark = cell(0);
    painter.circle_filled(Pos2::new(mark.center().x, rect.center().y), 3.5, state.ink(inks));
    let id = |column: usize| Id::unique(("missing-cell", &row.file, column));
    let last = row.last_path.display().to_string();
    table::cell(ui, painter, cell(1), id(1), &name, font.clone(), inks.primary, Some(last.as_str()));
    table::cell(ui, painter, cell(2), id(2), &row.uses(), font.clone(), inks.secondary, None);
    table::cell(ui, painter, cell(3), id(3), &state.word(), font.clone(), state.ink(inks), None);
    match &row.chosen {
        Some(path) => {
            table::path_cell(ui, painter, cell(4), id(4), &path.display().to_string(), font, inks.hint);
        }
        None => {
            table::cell(ui, painter, cell(4), id(4), "—", font, inks.hint, None);
        }
    }
    let (left, right) = spans[5];
    let button =
        Rect::from_min_max(Pos2::new(left + 2.0, rect.top() + 2.0), Pos2::new(right - 4.0, rect.bottom() - 2.0));
    let id = Id::unique(("missing-locate", &row.file));
    if widgets::button_with_id(ui, painter, button, id, "Locate…", &format!("Locate {name}"), false).clicked() {
        asked.locate = Some(row.file.clone());
    }
}

/// The panel's colours, read once a frame.
struct Inks {
    primary: Color32,
    secondary: Color32,
    hint: Color32,
    accent: Color32,
    error: Color32,
    ok: Color32,
    warning: Color32,
    line: Color32,
    strong_line: Color32,
    picked: Color32,
    hovered: Color32,
}

impl Inks {
    fn read() -> Self {
        use crate::kit::element::{self as el, BUTTON, LABEL, LIST_ROW, Selected};
        let row = || LIST_ROW.attribute("kind", "missing-file");
        Self {
            primary: el::name_ink(),
            secondary: el::caption_ink(),
            hint: el::hint_ink(),
            accent: el::accent_ink(),
            error: el::error_ink(),
            ok: LABEL.attribute("kind", "ok").look().color(),
            warning: LABEL.attribute("kind", "warning").look().color(),
            line: el::row_line(),
            strong_line: BUTTON.look().border(),
            picked: row().attr(Selected, true).look().background(),
            hovered: row().pointer(true, false, false).look().background(),
        }
    }
}

/// A section heading of the panel at the right: small, in capitals, quiet.
fn heading(painter: &egui::Painter, rect: Rect, text: &str, inks: &Inks) {
    let tiny = theme::ui_font(theme::TEXT_TINY);
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, &text.to_uppercase(), tiny, inks.hint);
}

/// `text` wrapped to `width` from (`left`, `top`); returns the bottom of what it drew.
fn wrapped(painter: &egui::Painter, left: f32, top: f32, width: f32, text: &str, font: FontId, color: Color32) -> f32 {
    let galley = painter.layout(text.to_string(), font, color, width.max(1.0));
    let bottom = top + galley.size().y;
    widgets::paint_text(painter, Pos2::new(left, top), galley, color);
    bottom
}

/// A state's tag at `at`: its word in `ink` on a faint fill of it. Returns its right edge.
fn tag(ui: &Ui, painter: &egui::Painter, at: Pos2, text: &str, ink: Color32) -> f32 {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let width = widgets::text_width(ui, text, small.clone()) + 16.0;
    let rect = Rect::from_min_size(at, Vec2::new(width, 20.0));
    painter.rect_filled(rect, 3.0, ink.gamma_multiply(0.16));
    painter.rect_stroke(rect, 3.0, theme::hairline(ink.gamma_multiply(0.5)), egui::StrokeKind::Inside);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, small, ink);
    rect.right()
}

/// A link: text in the accent, underlined. True when clicked.
fn link(ui: &Ui, painter: &egui::Painter, rect: Rect, id: Id, text: &str, label: &str, inks: &Inks) -> bool {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let width = widgets::text_width(ui, text, small.clone()).min(rect.width());
    let at = Rect::from_min_size(rect.min, Vec2::new(width, rect.height()));
    let response = widgets::button_area(ui, at, id, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    let ink = crate::kit::motion::mix(inks.accent, inks.primary, widgets::hover(ui, &response));
    widgets::text_in(painter, at, Align2::LEFT_CENTER, text, small, ink);
    painter.hline(at.x_range(), at.center().y + 7.0, egui::Stroke::new(1.0, ink));
    response.clicked()
}

/// "Show in Explorer" on Windows, "Show in Finder" on macOS.
fn show_in_folder_label() -> &'static str {
    if cfg!(target_os = "windows") {
        "Show in Explorer"
    } else if cfg!(target_os = "macos") {
        "Show in Finder"
    } else {
        "Show in folder"
    }
}

/// The path in pasted text: its first line, without the quotes a file manager's "copy as
/// path" puts round it.
fn pasted_path(text: &str) -> Option<PathBuf> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_matches(|c| c == '"' || c == '\'').trim();
    (!line.is_empty()).then(|| PathBuf::from(line))
}

/// Where the folder `last` was in moved to, by `now`: the two paths less the folders and the
/// name they end in alike (compared without regard to case). None when not even the name is
/// alike, or nothing is left of either.
fn moved_folder(last: &Path, now: &Path) -> Option<(PathBuf, PathBuf)> {
    let old: Vec<Component<'_>> = last.components().collect();
    let new: Vec<Component<'_>> = now.components().collect();
    let alike = |a: &Component<'_>, b: &Component<'_>| {
        a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
    };
    let same = old.iter().rev().zip(new.iter().rev()).take_while(|(a, b)| alike(a, b)).count();
    if same == 0 || same >= old.len() || same >= new.len() {
        return None;
    }
    let from: PathBuf = old.iter().take(old.len() - same).collect();
    let to: PathBuf = new.iter().take(new.len() - same).collect();
    (from != to).then_some((from, to))
}

/// A fingerprint of the audio files `project` names, clip by clip and device by device: it
/// changes when a clip or sampler gets, loses or changes a file, not with other edits.
fn audio_files_key(project: &suhina_engine::model::Project) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Inside device groups too: a grouped sampler's file is one the project names.
    for (_, device) in project.channels.iter().flat_map(|c| suhina_engine::model::walk_devices(&c.devices)) {
        device.sample_file().hash(&mut hasher);
        device.sample_absolute_file().hash(&mut hasher);
    }
    for content in &project.contents {
        for lane in &content.lanes {
            if let suhina_engine::model::LaneData::Audio(region) = &lane.data {
                region.file.hash(&mut hasher);
                region.absolute_file.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

#[cfg(test)]
#[path = "missing_files_tests.rs"]
mod tests;
