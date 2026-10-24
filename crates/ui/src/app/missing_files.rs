// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Missing files (`docs/spec/missing-files.md`): the bar that says a project's audio files are
//! missing, and the panel that finds them by name and relinks them.

use crate::kit::surface::Backgrounds as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};

use egui::{Align2, Color32, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use suhina_library::find::{Found, Walked, rank_candidates, walk_for_names};
use suhina_session::{ProjectId, Projects};

use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets;

const WIDTH: f32 = 600.0;
const LINE: f32 = 18.0;
/// A row: the name, the old path and the candidate, with their spacing.
const ROW_HEIGHT: f32 = 3.0 * LINE + 22.0;
const BUTTON: Vec2 = Vec2::new(150.0, 18.0);

/// One missing file and what may replace it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// As the project's clips name it.
    pub file: String,
    pub last_path: PathBuf,
    pub clips: usize,
    /// Files with its name, best first.
    pub candidates: Vec<PathBuf>,
    /// The one to link to, if any.
    pub chosen: Option<PathBuf>,
    /// The user chose it, so searches leave it.
    pub by_hand: bool,
    /// The path typed into the row's locate field.
    pub locate: String,
}

impl Row {
    fn name(&self) -> String {
        Path::new(&self.file)
            .file_name()
            .or_else(|| self.last_path.file_name())
            .map_or_else(|| self.file.clone(), |n| n.to_string_lossy().into_owned())
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
}

/// A folder walked on another thread.
struct Walk {
    stop: Arc<AtomicBool>,
    walked: Arc<AtomicUsize>,
    done: Receiver<Result<Walked, String>>,
    folder: PathBuf,
}

/// What the window does after the panel.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Relink these files (the clips' name, the new file).
    Apply(Vec<(String, PathBuf)>),
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
    pub rows: Vec<Row>,
    /// The bar was dismissed with Later for this project.
    pub bar_hidden: bool,
    pub open: bool,
    /// The folder typed into the panel's search field.
    pub folder: String,
    walk: Option<Walk>,
    pub note: Option<String>,
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
        if self.checked == Some((active, revision)) {
            return;
        }
        let new_project = self.checked.is_none_or(|(id, _)| id != active);
        if new_project {
            self.bar_hidden = false;
            self.open = false;
            self.rows.clear();
            self.note = None;
        }
        self.checked = Some((active, revision));
        let Some(session) = projects.get(active) else { return };
        let key = audio_files_key(session.project());
        if !new_project && self.files_key == Some(key) {
            return;
        }
        self.files_key = Some(key);
        let missing = session.missing_files();
        let project_folder = session.audio_folder().map(Path::to_path_buf);
        // Keep what was found or chosen for files still missing.
        let old = std::mem::take(&mut self.rows);
        self.rows = missing
            .into_iter()
            .map(|m| {
                old.iter().find(|r| r.file == m.file).cloned().unwrap_or(Row {
                    file: m.file,
                    last_path: m.last_path,
                    clips: m.clips,
                    candidates: Vec::new(),
                    chosen: None,
                    by_hand: false,
                    locate: String::new(),
                })
            })
            .collect();
        if self.rows.is_empty() {
            self.open = false;
            return;
        }
        let names = self.names();
        let found = projects.file_library_mut().find_by_names(&names);
        self.take(found);
        // The project's own folder is walked too, on a thread of its own.
        if new_project
            && let Some(folder) = project_folder.filter(|f| f.is_dir())
            && self.walk.is_none()
        {
            self.start_walk(folder);
        }
    }

    fn names(&self) -> Vec<String> {
        self.rows.iter().filter(|r| r.chosen.is_none() || !r.by_hand).map(Row::name).collect()
    }

    fn take(&mut self, found: Found) {
        for (name, paths) in found {
            for row in self.rows.iter_mut().filter(|r| r.name().eq_ignore_ascii_case(&name)) {
                row.offer(paths.clone());
            }
        }
    }

    fn start_walk(&mut self, folder: PathBuf) {
        let names = self.names();
        let stop = Arc::new(AtomicBool::new(false));
        let walked = Arc::new(AtomicUsize::new(0));
        let (send, done) = channel();
        let (s, w, f) = (Arc::clone(&stop), Arc::clone(&walked), folder.clone());
        let started = std::thread::Builder::new().name("suhina-find-files".into()).spawn(move || {
            let _ = send.send(walk_for_names(&f, &names, &s, &w));
        });
        if started.is_ok() {
            self.note = None;
            self.walk = Some(Walk { stop, walked, done, folder });
        }
    }

    /// Takes in a finished walk. Returns whether one is still going.
    fn poll(&mut self) -> bool {
        let Some(walk) = &self.walk else { return false };
        match walk.done.try_recv() {
            Ok(Ok(walked)) => {
                let folder = walk.folder.display().to_string();
                self.walk = None;
                let count = walked.found.iter().filter(|(_, p)| !p.is_empty()).count();
                self.note = Some(match walked.note {
                    Some(note) => format!("{folder}: {note}"),
                    None => format!("{folder}: {count} found"),
                });
                self.take(walked.found);
                false
            }
            Ok(Err(e)) => {
                self.walk = None;
                self.note = Some(e);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.walk = None;
                false
            }
        }
    }

    /// The files that have a candidate, for relinking.
    fn chosen(&self) -> Vec<(String, PathBuf)> {
        self.rows.iter().filter_map(|r| Some((r.file.clone(), r.chosen.clone()?))).collect()
    }

    /// Draws the bar that says files are missing in `rect`, while it should show.
    pub fn bar(&mut self, ui: &mut Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.fill_background(rect, 0.0, theme::panel_background());
        let count = self.rows.len();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(Vec2::new(PAD, 6.0))), |ui| {
            ui.horizontal_centered(|ui| {
                let text = format!("{count} audio file{} missing", if count == 1 { " is" } else { "s are" });
                ui.label(egui::RichText::new(text).color(theme::text_primary()));
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
        egui::Area::new(Id::new("missing-files"))
            .order(egui::Order::Foreground)
            .fixed_pos(area.min)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(area.size());
                self.draw(ui, area, project)
            })
            .inner
    }

    fn draw(&mut self, ui: &mut Ui, area: Rect, project: &str) -> Option<Outcome> {
        let behind = ui.interact(area, Id::new("missing-files-behind"), Sense::click_and_drag());
        let _swallowed = behind.clicked();
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(0.6));
        let height = ROW + PAD * 4.0 + LINE * 3.0 + self.rows.len() as f32 * ROW_HEIGHT + BUTTON.y;
        let size = Vec2::new(WIDTH.min(area.width() - 2.0 * PAD), height.min(area.height() - 2.0 * PAD));
        let rect = Rect::from_center_size(area.center(), size);
        crate::kit::liquid::backdrop(&painter, rect);
        painter.fill_background(rect, 0.0, theme::panel_background());
        painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
        painter.fill_background(bar, 0.0, theme::control_background());
        widgets::text_in(
            &painter,
            bar.shrink2(Vec2::new(PAD, 0.0)),
            Align2::LEFT_CENTER,
            &format!("Missing files · {project}"),
            theme::ui_font(theme::TEXT),
            theme::text_primary(),
        );

        let mut outcome = None;
        let inner = Rect::from_min_max(Pos2::new(rect.left() + PAD, bar.bottom() + PAD), rect.max - Vec2::splat(PAD));
        let small = theme::ui_font(theme::TEXT_SMALL);
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // Searching.
            ui.horizontal(|ui| {
                if ui.button("Search the browser's folders").clicked() {
                    self.checked = None;
                }
                ui.label(egui::RichText::new("or a folder:").color(theme::text_secondary()).font(small.clone()));
                let field = crate::kit::widgets::text_input(
                    ui,
                    egui::TextEdit::singleline(&mut self.folder)
                        .font(small.clone())
                        .hint_text("A folder path")
                        .desired_width(220.0),
                );
                field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Folder to search"));
                let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                match &self.walk {
                    Some(walk) => {
                        let n = walk.walked.load(Ordering::Relaxed);
                        ui.label(egui::RichText::new(format!("Searched {n} files")).font(small.clone()));
                        if ui.button("Stop").clicked() {
                            walk.stop.store(true, Ordering::Relaxed);
                        }
                    }
                    None => {
                        if (ui.button("Search").clicked() || entered) && !self.folder.trim().is_empty() {
                            self.start_walk(PathBuf::from(self.folder.trim()));
                        }
                    }
                }
            });
            // A folder dropped onto the panel is searched.
            let dropped: Option<PathBuf> = ui.input(|i| {
                i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).find(|p| !p.as_os_str().is_empty())
            });
            if let Some(folder) = dropped.filter(|p| p.is_dir())
                && self.walk.is_none()
            {
                self.folder = folder.display().to_string();
                self.start_walk(folder);
            }
            let found = self.rows.iter().filter(|r| r.chosen.is_some()).count();
            let note = match &self.note {
                Some(note) => format!("{found} of {} found · {note}", self.rows.len()),
                None => format!("{found} of {} found", self.rows.len()),
            };
            ui.label(egui::RichText::new(note).color(theme::text_secondary()).font(small.clone()));
            ui.separator();

            let list_height = (inner.height() - LINE * 3.0 - BUTTON.y - PAD * 2.0).max(ROW_HEIGHT);
            egui::ScrollArea::vertical().max_height(list_height).auto_shrink([false, true]).show(ui, |ui| {
                for (i, row) in self.rows.iter_mut().enumerate() {
                    row_view(ui, i, row, &small);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                let chosen = self.chosen();
                let apply = ui.add_enabled(
                    !chosen.is_empty(),
                    egui::Button::new(format!("Apply ({})", chosen.len())).min_size(BUTTON),
                );
                if apply.on_hover_text("Point the clips at the files found, one undo step").clicked() {
                    outcome = Some(Outcome::Apply(chosen));
                }
                if ui.add(egui::Button::new("Close").min_size(BUTTON)).clicked() {
                    self.close();
                }
            });
        });
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            self.close();
        }
        if outcome.is_some() {
            self.close();
        }
        outcome
    }

    fn close(&mut self) {
        self.open = false;
        if let Some(walk) = &self.walk {
            walk.stop.store(true, Ordering::Relaxed);
        }
    }
}

fn row_view(ui: &mut Ui, index: usize, row: &mut Row, small: &egui::FontId) {
    let (state, colour): (&str, Color32) = match (&row.chosen, row.candidates.len()) {
        (None, _) => ("missing", theme::recording()),
        (Some(_), n) if n > 1 => ("several found", theme::text_value()),
        (Some(_), _) => ("found", theme::accent()),
    };
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 3.5, colour);
        ui.label(egui::RichText::new(row.name()).color(theme::text_primary())).on_hover_text(state);
        let clips = format!("{} clip{}", row.clips, if row.clips == 1 { "" } else { "s" });
        ui.label(egui::RichText::new(clips).color(theme::text_hint()).font(small.clone()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let field = crate::kit::widgets::text_input(
                ui,
                egui::TextEdit::singleline(&mut row.locate)
                    .font(small.clone())
                    .hint_text("Locate: a file path")
                    .desired_width(170.0),
            );
            field.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, format!("Locate {}", row.name()))
            });
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                let path = PathBuf::from(row.locate.trim());
                if path.is_file() {
                    if !row.candidates.contains(&path) {
                        row.candidates.insert(0, path.clone());
                    }
                    row.chosen = Some(path);
                    row.by_hand = true;
                    row.locate.clear();
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(
            egui::RichText::new(row.last_path.display().to_string()).color(theme::text_hint()).font(small.clone()),
        );
    });
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(egui::RichText::new("→").color(theme::text_hint()).font(small.clone()));
        let shown = row.chosen.as_ref().map_or_else(|| "not found".to_string(), |p| p.display().to_string());
        crate::kit::widgets::combo_box(
            ui,
            egui::ComboBox::from_id_salt(("missing-candidate", index))
                .selected_text(egui::RichText::new(shown).font(small.clone()))
                .width(440.0),
            |ui| {
                for candidate in row.candidates.clone() {
                    let label = candidate.display().to_string();
                    if ui.selectable_label(row.chosen.as_ref() == Some(&candidate), label).clicked() {
                        row.chosen = Some(candidate);
                        row.by_hand = true;
                    }
                }
                if ui.selectable_label(row.chosen.is_none(), "Leave it missing").clicked() {
                    row.chosen = None;
                    row.by_hand = true;
                }
            },
        );
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(file: &str, last: &str) -> Row {
        Row {
            file: file.into(),
            last_path: PathBuf::from(last),
            clips: 1,
            candidates: Vec::new(),
            chosen: None,
            by_hand: false,
            locate: String::new(),
        }
    }

    #[test]
    fn the_best_candidate_is_chosen_unless_the_user_chose() {
        let mut r = row("kick.wav", "/old/Drums/kick.wav");
        r.offer(vec![PathBuf::from("/new/Other/kick.wav"), PathBuf::from("/new/Drums/kick.wav")]);
        assert_eq!(r.chosen, Some(PathBuf::from("/new/Drums/kick.wav")));
        r.chosen = Some(PathBuf::from("/new/Other/kick.wav"));
        r.by_hand = true;
        r.offer(vec![PathBuf::from("/x/Drums/kick.wav")]);
        assert_eq!(r.chosen, Some(PathBuf::from("/new/Other/kick.wav")));
        assert_eq!(r.candidates.len(), 3);
        assert_eq!(r.name(), "kick.wav");
    }

    #[test]
    fn searches_give_rows_their_candidates_and_the_chosen_ones_are_applied() {
        let mut missing = MissingFiles {
            rows: vec![row("a/Kick.wav", "/old/a/Kick.wav"), row("snare.wav", "/old/snare.wav")],
            ..MissingFiles::default()
        };
        missing.take(vec![("kick.wav".into(), vec![PathBuf::from("/new/a/kick.wav")]), ("snare.wav".into(), vec![])]);
        assert_eq!(missing.chosen(), vec![("a/Kick.wav".to_string(), PathBuf::from("/new/a/kick.wav"))]);
        assert!(missing.bar_shows());
        missing.bar_hidden = true;
        assert!(!missing.bar_shows());
    }

    /// A sampler inside a device group names its file as one on the track does: changing it
    /// changes the key the list is rebuilt by.
    #[test]
    fn a_grouped_sampler_s_file_is_in_the_key() {
        use suhina_engine::model::{Branch, Device, DeviceGroup, DeviceKind};
        let mut project = crate::demo::demo_project();
        let sampler = Device::new("Sampler", DeviceKind::Builtin("sampler".into()));
        let mut branch = Branch::new("Branch 1");
        branch.devices.push(sampler);
        project.channels[0].devices.push(Device::group("Group", DeviceGroup::with_branches(vec![branch])));
        let before = super::audio_files_key(&project);
        let group = project.channels[0].devices.last_mut().and_then(Device::as_group_mut).unwrap();
        group.branches[0].devices[0].set_sample_file("samples/Kick.wav");
        assert_ne!(super::audio_files_key(&project), before);
    }

    #[test]
    fn a_folder_walk_runs_in_the_background() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("suhina-missing-ui-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(dir.join("deep")).unwrap();
        std::fs::write(dir.join("deep").join("snare.wav"), b"x").unwrap();
        let mut missing = MissingFiles { rows: vec![row("snare.wav", "/old/snare.wav")], ..MissingFiles::default() };
        missing.start_walk(dir.clone());
        for _ in 0..200 {
            if !missing.poll() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(missing.rows[0].chosen, Some(dir.join("deep").join("snare.wav")));
        assert!(missing.note.as_deref().is_some_and(|n| n.ends_with("1 found")));
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod look {
    use super::*;

    /// Renders the bar and the panel to `target/missing-files.png` to look at (run with
    /// `--ignored`).
    #[test]
    #[ignore]
    fn render_the_panel() {
        use egui_kittest::Harness;
        let mut found = Row {
            file: "Drums/Kick 909.wav".into(),
            last_path: PathBuf::from("D:/Samples/Drums/Kick 909.wav"),
            clips: 3,
            candidates: vec![PathBuf::from("E:/Library/Drums/Kick 909.wav"), PathBuf::from("E:/Old/Kick 909.wav")],
            chosen: None,
            by_hand: false,
            locate: String::new(),
        };
        found.chosen = found.candidates.first().cloned();
        let one = Row {
            file: "Vox/Chop 2.wav".into(),
            last_path: PathBuf::from("D:/Samples/Vox/Chop 2.wav"),
            clips: 1,
            candidates: vec![PathBuf::from("E:/Library/Vox/Chop 2.wav")],
            chosen: Some(PathBuf::from("E:/Library/Vox/Chop 2.wav")),
            by_hand: false,
            locate: String::new(),
        };
        let lost = Row {
            file: "bass loop.wav".into(),
            last_path: PathBuf::from("D:/Projects/Song/bass loop.wav"),
            clips: 2,
            candidates: Vec::new(),
            chosen: None,
            by_hand: false,
            locate: String::new(),
        };
        let state = MissingFiles { rows: vec![found, one, lost], open: true, ..MissingFiles::default() };
        let mut harness = Harness::builder().with_size(egui::vec2(820.0, 520.0)).build_ui_state(
            |ui, state: &mut MissingFiles| {
                crate::kit::theme::install(ui.ctx());
                let area = ui.max_rect();
                ui.painter().rect_filled(area, 0.0, theme::window_background());
                let bar = Rect::from_min_size(area.min, Vec2::new(area.width(), theme::ROW + 12.0));
                state.bar(ui, bar);
                state.open = true;
                let _ = state.panel(ui, area, "Song");
            },
            state,
        );
        harness.run();
        harness.render().unwrap().save("../../target/missing-files.png").unwrap();
    }
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
