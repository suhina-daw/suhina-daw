// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plug-in manager (`docs/spec/plugin-manager.md`): every plug-in the scans found and every
//! file that gave none, in one table sortable by its columns, searched by name, vendor or file.
//! A plug-in is switched on or off by its checkbox; a file that failed, or that is built for
//! another processor, is read again with its **Scan again**. Opened from Settings › Plug-ins and
//! drawn in the Settings dialog's place until it is closed; what it changes goes through the
//! `plugin.*` commands, as [`PluginRequest`]s the app runs.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::kit::theme::{self, PAD};
use crate::kit::{motion, widgets};
use crate::settings::settings_view::{PluginRequest, PluginsView};

/// One row of the table.
const ROW: f32 = 20.0;
/// The header with the title, the search and the buttons.
const TOP: f32 = 56.0;
/// The columns, by index: on, name, vendor, type, format, version, status, file, action. The
/// name and the file share what the fixed ones leave.
const COLUMNS: [&str; 9] = ["On", "Name", "Vendor", "Type", "Format", "Version", "Status", "File", ""];
const FIXED: [f32; 9] = [28.0, 0.0, 130.0, 72.0, 52.0, 70.0, 96.0, 0.0, 84.0];
/// The name's share of the room left; the file takes the rest.
const NAME_SHARE: f32 = 0.42;

/// What the manager keeps between frames: whether it shows, the search, and the sort.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagerState {
    pub open: bool,
    pub search: String,
    /// The column sorted by (an index into the columns) and whether descending.
    pub sort: (usize, bool),
    /// The plug-ins' rows picked, by spec, and the one picked last, which Shift picks the run
    /// from (#725).
    pub picked: Vec<String>,
    pub anchor: Option<String>,
}

impl ManagerState {
    /// Picks the row of `spec` the way a click does: alone, with Ctrl (Cmd) added or taken out,
    /// with Shift the run from the last one picked in the order shown (`order`).
    pub fn pick(&mut self, spec: &str, command: bool, shift: bool, order: &[String]) {
        if shift
            && let Some(anchor) = &self.anchor
            && let (Some(from), Some(to)) =
                (order.iter().position(|s| s == anchor), order.iter().position(|s| s == spec))
        {
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            self.picked = order.get(from..=to).map(<[String]>::to_vec).unwrap_or_default();
            return;
        }
        if command {
            match self.picked.iter().position(|s| s == spec) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(spec.to_string()),
            }
        } else {
            self.picked = vec![spec.to_string()];
        }
        self.anchor = Some(spec.to_string());
    }
}

/// The plug-ins a right-click on the row of `spec` turns off or on: the picked ones the list
/// shows (`order`), when that row is one of them, in the order they were picked; else that row
/// alone. A picked row the search hides, or a rescan removed, is left alone.
fn batch(picked: &[String], spec: &str, order: &[String]) -> Vec<String> {
    if picked.iter().any(|s| s == spec) {
        let shown: std::collections::HashSet<&str> = order.iter().map(String::as_str).collect();
        picked.iter().filter(|s| shown.contains(s.as_str())).cloned().collect()
    } else {
        vec![spec.to_string()]
    }
}

/// What a row stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A plug-in, by index into [`PluginsView::found`].
    Plugin(usize),
    /// A file that failed to load, by index into [`PluginsView::failed`].
    Failed(usize),
    /// A file built for another processor, by index into [`PluginsView::incompatible`].
    Incompatible(usize),
}

/// The texts of a row's columns, and what it stands for.
struct Line {
    kind: Kind,
    cells: [String; 9],
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// Every row, the search applied, sorted.
fn lines(plugins: &PluginsView, state: &ManagerState) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for (index, p) in plugins.found.iter().enumerate() {
        let status = if p.enabled { "on" } else { "switched off" };
        let cells = [
            String::new(),
            p.name.clone(),
            p.vendor.clone(),
            p.kind.clone(),
            p.format.clone(),
            p.version.clone(),
            status.into(),
            p.file.clone(),
            String::new(),
        ];
        lines.push(Line { kind: Kind::Plugin(index), cells });
    }
    let others = plugins
        .failed
        .iter()
        .enumerate()
        .map(|(i, f)| (Kind::Failed(i), f, "failed"))
        .chain(plugins.incompatible.iter().enumerate().map(|(i, f)| (Kind::Incompatible(i), f, "other processor")));
    for (kind, (file, why), status) in others {
        let format = std::path::Path::new(file)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let cells = [
            String::new(),
            file_name(file),
            String::new(),
            String::new(),
            format,
            String::new(),
            status.into(),
            why.clone(),
            String::new(),
        ];
        lines.push(Line { kind, cells });
    }
    let words: Vec<String> = state.search.split_whitespace().map(str::to_lowercase).collect();
    lines.retain(|line| {
        let text = format!("{} {} {}", line.cells[1], line.cells[2], line.cells[7]).to_lowercase();
        words.iter().all(|w| text.contains(w.as_str()))
    });
    let (column, descending) = state.sort;
    lines.sort_by(|a, b| {
        let key = |line: &Line| line.cells.get(column).cloned().unwrap_or_default().to_lowercase();
        let order =
            if column == 1 { suhina_library::natural_cmp(&a.cells[1], &b.cells[1]) } else { key(a).cmp(&key(b)) };
        order.then_with(|| suhina_library::natural_cmp(&a.cells[1], &b.cells[1]))
    });
    if descending {
        lines.reverse();
    }
    lines
}

/// The left and right edge of each column across `area`.
fn spans(area: Rect) -> [(f32, f32); 9] {
    let fixed: f32 = FIXED.iter().sum();
    let room = (area.width() - fixed).max(0.0);
    let mut widths = FIXED;
    widths[1] = (room * NAME_SHARE).max(80.0);
    widths[7] = (room - widths[1]).max(0.0);
    let mut x = area.left();
    let mut spans = [(0.0, 0.0); 9];
    for (span, width) in spans.iter_mut().zip(widths) {
        *span = (x, x + width);
        x += width;
    }
    spans
}

/// What the manager asks for this frame.
#[derive(Debug, Default)]
pub struct ManagerOut {
    pub request: Option<PluginRequest>,
    /// Close was clicked (or Escape pressed): Settings shows again.
    pub close: bool,
}

/// Draws the manager over `area`.
pub fn show(ui: &mut Ui, area: Rect, plugins: &mut PluginsView) -> ManagerOut {
    let mut out = ManagerOut::default();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    let inner = area.shrink(2.0 * PAD);
    let small = theme::ui_font(theme::TEXT_SMALL);

    // The title and the counts, the search, then Scan, Rescan all and Close on the right.
    let title = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 20.0));
    widgets::text_in(
        &painter,
        title,
        Align2::LEFT_CENTER,
        "Plug-in manager",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let off = plugins.found.iter().filter(|p| !p.enabled).count();
    let counts = format!(
        "{} plug-ins ({off} switched off), {} files failed, {} built for another processor",
        plugins.found.len(),
        plugins.failed.len(),
        plugins.incompatible.len()
    );
    let note = Rect::from_min_max(
        Pos2::new(inner.left() + 130.0, title.top()),
        Pos2::new(inner.right() - 250.0, title.bottom()),
    );
    widgets::text_in(&painter, note, Align2::LEFT_CENTER, &counts, small.clone(), theme::text_secondary());
    let mut x = inner.right();
    for (text, label) in
        [("Close", "Close the plug-in manager"), ("Rescan all", "Rescan all plug-ins"), ("Scan", "Scan for plug-ins")]
    {
        let width = widgets::text_width(ui, text, small.clone()) + 20.0;
        x -= width;
        let rect = Rect::from_min_size(Pos2::new(x, title.top()), Vec2::new(width, 18.0));
        x -= 4.0;
        let busy = plugins.scanning && text != "Close";
        if widgets::button(ui, &painter, rect, text, label) && !busy {
            match text {
                "Close" => out.close = true,
                "Scan" => out.request = Some(PluginRequest::Scan { all: false }),
                _ => out.request = Some(PluginRequest::Scan { all: true }),
            }
        }
    }
    let search = Rect::from_min_size(Pos2::new(inner.left(), title.bottom() + 8.0), Vec2::new(260.0, 20.0));
    let edit = ui.put(
        search,
        egui::TextEdit::singleline(&mut plugins.manager.search)
            .hint_text("Find by name, vendor or file")
            .font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Find a plug-in"));
    if plugins.scanning {
        let hint = Rect::from_min_max(
            Pos2::new(search.right() + 12.0, search.top()),
            Pos2::new(inner.right(), search.bottom()),
        );
        widgets::text_in(&painter, hint, Align2::LEFT_CENTER, "Scanning…", small.clone(), theme::text_hint());
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !edit.has_focus() {
        out.close = true;
    }

    // The column headings: a click sorts by one, again the other way.
    let header = Rect::from_min_size(Pos2::new(inner.left(), inner.top() + TOP), Vec2::new(inner.width(), ROW));
    let columns = spans(header);
    painter.hline(header.x_range(), header.bottom() - 0.5, theme::hairline(theme::border()));
    for (index, (name, (left, right))) in COLUMNS.iter().zip(columns).enumerate() {
        if name.is_empty() || right - left < 8.0 {
            continue;
        }
        let rect = Rect::from_min_max(Pos2::new(left, header.top()), Pos2::new(right, header.bottom()));
        let response =
            widgets::button_area(ui, rect, Id::new(("manager-column", index)), format!("Sort the plug-ins by {name}"));
        if response.clicked() {
            let (sorted, descending) = plugins.manager.sort;
            plugins.manager.sort = (index, if sorted == index { !descending } else { false });
        }
        let (sorted, descending) = plugins.manager.sort;
        let label = if sorted == index {
            format!("{name} {}", if descending { "▼" } else { "▲" })
        } else {
            (*name).to_string()
        };
        let color = if sorted == index { theme::text_primary() } else { theme::text_hint() };
        let fill = motion::mix(theme::window_background(), theme::panel_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, fill);
        widgets::text_in(
            &painter,
            rect.shrink2(Vec2::new(3.0, 0.0)),
            Align2::LEFT_CENTER,
            &label,
            small.clone(),
            color,
        );
    }

    // The rows, only those in view drawn.
    let body = Rect::from_min_max(Pos2::new(inner.left(), header.bottom() + 2.0), inner.max);
    let lines = lines(plugins, &plugins.manager);
    if lines.is_empty() {
        let text = if plugins.manager.search.trim().is_empty() {
            "No plug-ins found yet: Scan looks for them"
        } else {
            "Nothing found"
        };
        widgets::text_in(&painter, body.shrink(PAD), Align2::CENTER_TOP, text, small.clone(), theme::text_hint());
        return out;
    }
    let mut toggled: Option<(String, bool)> = None;
    let mut toggled_many: Option<(Vec<String>, bool)> = None;
    let mut pick: Option<(String, bool, bool)> = None;
    let order: Vec<String> = lines
        .iter()
        .filter_map(|l| match l.kind {
            Kind::Plugin(i) => plugins.found.get(i).map(|p| p.spec.clone()),
            _ => None,
        })
        .collect();
    let picked = plugins.manager.picked.clone();
    let scanning = plugins.scanning;
    let mut again: Option<String> = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW, lines.len(), |ui, range| {
            for line in lines.get(range).unwrap_or_default() {
                let (rect, row) = ui.allocate_exact_size(Vec2::new(body.width(), ROW), Sense::click());
                let columns = spans(rect);
                let row_painter = ui.painter_at(rect);
                let spec = match line.kind {
                    Kind::Plugin(i) => plugins.found.get(i).map(|p| p.spec.clone()),
                    _ => None,
                };
                let lit = spec.as_ref().is_some_and(|s| picked.contains(s));
                if lit {
                    row_painter.rect_filled(rect, 0.0, theme::control_active());
                } else if ui.rect_contains_pointer(rect) {
                    row_painter.rect_filled(rect, 0.0, theme::panel_hover());
                }
                // A click picks the row (Ctrl adds, Shift takes the run); the right-click menu
                // switches every picked plug-in off or on (#725).
                if let Some(spec) = &spec {
                    if row.clicked() {
                        let (command, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
                        pick = Some((spec.clone(), command, shift));
                    }
                    if row.secondary_clicked() && !picked.contains(spec) {
                        pick = Some((spec.clone(), false, false));
                    }
                    // Worked out only while the menu is open, not for every row every frame.
                    row.context_menu(|ui| {
                        let these = batch(&picked, spec, &order);
                        let what =
                            if these.len() > 1 { format!("{} plug-ins", these.len()) } else { "the plug-in".into() };
                        if ui.add_enabled(!scanning, egui::Button::new(format!("Turn off {what}"))).clicked() {
                            toggled_many = Some((these.clone(), false));
                            ui.close();
                        }
                        if ui.add_enabled(!scanning, egui::Button::new(format!("Turn on {what}"))).clicked() {
                            toggled_many = Some((these.clone(), true));
                            ui.close();
                        }
                    });
                }
                for (index, ((left, right), text)) in columns.iter().zip(&line.cells).enumerate() {
                    let cell =
                        Rect::from_min_max(Pos2::new(left + 3.0, rect.top()), Pos2::new(right - 3.0, rect.bottom()));
                    if cell.width() <= 0.0 || text.is_empty() {
                        continue;
                    }
                    let color = match (index, line.kind) {
                        (1, Kind::Plugin(i)) if plugins.found.get(i).is_some_and(|p| p.enabled) => theme::text_value(),
                        (6 | 7, Kind::Failed(_)) => theme::recording(),
                        (1, _) => theme::text_secondary(),
                        _ => theme::text_hint(),
                    };
                    let galley = widgets::fitted(&row_painter, text, small.clone(), color, cell.width());
                    row_painter.galley(Pos2::new(cell.left(), rect.center().y - galley.size().y / 2.0), galley, color);
                }
                let (on_left, on_right) = columns[0];
                let (action_left, action_right) = columns[8];
                match line.kind {
                    Kind::Plugin(i) => {
                        let Some(plugin) = plugins.found.get(i) else { continue };
                        let mut enabled = plugin.enabled;
                        let check = Rect::from_center_size(
                            Pos2::new((on_left + on_right) / 2.0, rect.center().y),
                            Vec2::splat(16.0),
                        );
                        // Off while a scan runs: its result replaces the library, switches too.
                        let response = ui.put(check, |ui: &mut egui::Ui| {
                            ui.add_enabled(!scanning, egui::Checkbox::without_text(&mut enabled))
                        });
                        let label = format!("Use {}", plugin.name);
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, plugin.enabled, &label)
                        });
                        if response.changed() {
                            toggled = Some((plugin.spec.clone(), enabled));
                        }
                    }
                    Kind::Failed(i) | Kind::Incompatible(i) => {
                        let list =
                            if matches!(line.kind, Kind::Failed(_)) { &plugins.failed } else { &plugins.incompatible };
                        let Some((file, _)) = list.get(i) else { continue };
                        let button = Rect::from_min_max(
                            Pos2::new(action_left + 2.0, rect.top() + 1.0),
                            Pos2::new(action_right, rect.bottom() - 1.0),
                        );
                        let label = format!("Scan again {}", file_name(file));
                        if !plugins.scanning && widgets::button(ui, &row_painter, button, "Scan again", &label) {
                            again = Some(file.clone());
                        }
                    }
                }
            }
        });
    });
    if let Some((spec, command, shift)) = pick {
        plugins.manager.pick(&spec, command, shift, &order);
    }
    if let Some((specs, enabled)) = toggled_many {
        out.request = Some(PluginRequest::SetEnabledMany(specs, enabled));
    } else if let Some((spec, enabled)) = toggled {
        out.request = Some(PluginRequest::SetEnabled(spec, enabled));
    } else if let Some(file) = again {
        out.request = Some(PluginRequest::ScanFile(file));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_picked_alone_with_ctrl_and_as_a_run_with_shift() {
        let order: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let mut state = ManagerState::default();
        state.pick("b", false, false, &order);
        state.pick("d", true, false, &order);
        assert_eq!(state.picked, ["b", "d"]);
        state.pick("b", true, false, &order);
        assert_eq!(state.picked, ["d"], "Ctrl takes a picked row out");
        state.pick("a", false, true, &order);
        assert_eq!(state.picked, ["a", "b"], "Shift takes the run from the row clicked last");
        state.pick("c", false, false, &order);
        assert_eq!(state.picked, ["c"], "a plain click picks that row only");
    }

    #[test]
    fn a_right_click_turns_the_picked_rows_in_view_on_or_off() {
        let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let picked = strings(&["b", "gone", "d", "hidden"]);
        // The search shows a to d; "hidden" is out of it, "gone" went in a rescan.
        let order = strings(&["a", "b", "c", "d"]);
        assert_eq!(batch(&picked, "d", &order), ["b", "d"]);
        assert_eq!(batch(&picked, "c", &order), ["c"], "a row not picked: that one alone");
    }
    use crate::settings::settings_view::PluginRow;

    fn view() -> PluginsView {
        let plugin = |name: &str, vendor: &str, enabled: bool| PluginRow {
            name: name.into(),
            vendor: vendor.into(),
            kind: "effect".into(),
            format: "vst3".into(),
            version: "1".into(),
            file: format!("/vst3/{name}.vst3"),
            spec: format!("vst3:/vst3/{name}.vst3#00"),
            enabled,
        };
        PluginsView {
            found: vec![plugin("Verb 10", "Zeta", true), plugin("Verb 9", "Alpha", false)],
            failed: vec![("/vst3/Broken.vst3".into(), "the plug-in has no factory".into())],
            incompatible: vec![("/vst3/Old.vst3".into(), "built for x86_64 only; Suhina runs on arm64".into())],
            ..PluginsView::default()
        }
    }

    #[test]
    fn the_table_lists_plugins_and_files_sorted_and_searched() {
        let plugins = view();
        let mut state = ManagerState { sort: (1, false), ..ManagerState::default() };
        let names: Vec<String> = lines(&plugins, &state).iter().map(|l| l.cells[1].clone()).collect();
        assert_eq!(names, ["Broken.vst3", "Old.vst3", "Verb 9", "Verb 10"], "natural order by name");
        state.sort = (2, false);
        let vendors: Vec<String> = lines(&plugins, &state).iter().map(|l| l.cells[2].clone()).collect();
        assert_eq!(vendors[2..], ["Alpha", "Zeta"]);
        state.sort = (6, true);
        let first = lines(&plugins, &state).into_iter().next().map(|l| l.cells[6].clone());
        assert_eq!(first.as_deref(), Some("switched off"), "status, descending");
        state.search = "alpha".into();
        assert_eq!(lines(&plugins, &state).len(), 1);
        state.search = "x86".into();
        let found = lines(&plugins, &state);
        assert_eq!((found.len(), found[0].kind), (1, Kind::Incompatible(0)), "the reason is searched too");
    }

    #[test]
    fn the_name_and_the_file_share_what_the_fixed_columns_leave() {
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, ROW));
        let spans = spans(area);
        assert_eq!(spans[0].0, 0.0);
        assert!((spans[8].1 - 1000.0).abs() < 0.01, "the columns fill the width");
        assert!(spans.windows(2).all(|w| (w[0].1 - w[1].0).abs() < 0.01), "no gaps");
        assert!(spans[1].1 - spans[1].0 > spans[7].1 - spans[7].0 - 200.0);
    }
}
