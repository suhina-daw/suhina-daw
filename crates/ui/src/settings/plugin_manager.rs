// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plug-in manager (`docs/spec/plugin-manager.md`): every plug-in the scans found and every
//! file that gave none. A library sidebar narrows the list (all, switched on, switched off, the
//! files that need attention) and filters it by type, format and vendor; the table in the middle
//! is searched and sorted, its columns dragged wider or narrower and kept so; the panel on the
//! right shows the row picked last. A plug-in is switched on or off by its checkbox; a file that
//! failed, or that is built for another processor, is read again with its **Scan again**. Opened
//! from Settings › Plug-ins and drawn in the Settings dialog's place until it is closed; what it
//! changes goes through the `plugin.*` commands, as [`PluginRequest`]s the app runs. Every colour
//! comes from the theme.

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::kit::theme::{self, PAD};
use crate::kit::{motion, widgets};
use crate::settings::settings_view::{PluginRequest, PluginsView};

/// One row of the table.
const ROW: f32 = 22.0;
/// The bar with the title and the scan buttons, and the status bar under everything.
const TOP: f32 = 40.0;
const STATUS: f32 = 24.0;
/// The library sidebar's and the selected plug-in's panel's widths, and the narrowest the table
/// is kept when they are both shown: a narrower dialog drops the panel, then the sidebar.
const SIDEBAR: f32 = 190.0;
const DETAIL: f32 = 270.0;
const TABLE_MIN: f32 = 420.0;
/// One item of the sidebar.
const ITEM: f32 = 24.0;

/// How many columns the table has (the UI settings keep one width each).
pub const COLUMN_COUNT: usize = 9;
/// The columns, by index: on, name, vendor, type, format, version, status, file, action.
const COLUMNS: [&str; COLUMN_COUNT] = ["On", "Name", "Vendor", "Type", "Format", "Version", "Status", "File", ""];
/// Each column's width at first; the file takes what the others leave.
const WIDTHS: [f32; COLUMN_COUNT] = [28.0, 220.0, 140.0, 84.0, 64.0, 72.0, 150.0, 0.0, 88.0];
/// The narrowest each column is dragged.
const MIN_WIDTHS: [f32; COLUMN_COUNT] = [28.0, 80.0, 48.0, 48.0, 48.0, 48.0, 64.0, 80.0, 88.0];
/// The widest a double-click fits a column to.
const FIT_MAX: f32 = 600.0;
/// The column the file's path or a file's reason is in, which fills the rest of the width.
const FILE: usize = 7;
/// The least the file column is given before the table scrolls sideways: room for a path.
const FILE_WANT: f32 = 320.0;
const ACTION: usize = 8;

/// Which rows the library sidebar shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Shelf {
    #[default]
    All,
    Enabled,
    Disabled,
    /// The plug-ins marked as owned.
    Owned,
    /// The files that failed and those built for another processor.
    Attention,
    Failed,
    Incompatible,
}

/// The type filter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TypeFilter {
    #[default]
    All,
    Effects,
    Instruments,
}

/// What the manager keeps between frames: whether it shows, the search, the sort, the filters,
/// the rows picked and the column widths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManagerState {
    pub open: bool,
    pub search: String,
    /// The column sorted by (an index into the columns) and whether descending.
    pub sort: (usize, bool),
    /// The rows picked, by key (a plug-in's spec, `file:` and a file's path), and the one picked
    /// last, which Shift picks the run from (#725) and the panel on the right shows.
    pub picked: Vec<String>,
    pub anchor: Option<String>,
    pub shelf: Shelf,
    pub types: TypeFilter,
    /// The formats ticked (`vst3`, `clap`, `vst2`); none ticked shows every format.
    pub formats: Vec<String>,
    pub vendor: Option<String>,
    /// The column widths as dragged, in points; a column with none (or 0) has its width at first.
    pub widths: Vec<u32>,
    /// The columns hidden, by heading (`Vendor`, `File` …), from the columns menu.
    pub hidden: Vec<String>,
    /// The middle shows the folders scans look in (**Scan locations**) instead of the table.
    pub locations: bool,
    /// The widths and the hidden columns were read from the UI settings.
    pub widths_read: bool,
}

impl ManagerState {
    /// Picks the row of `key` the way a click does: alone, with Ctrl (Cmd) added or taken out,
    /// with Shift the run from the last one picked in the order shown (`order`).
    pub fn pick(&mut self, key: &str, command: bool, shift: bool, order: &[String]) {
        if shift
            && let Some(anchor) = &self.anchor
            && let (Some(from), Some(to)) =
                (order.iter().position(|s| s == anchor), order.iter().position(|s| s == key))
        {
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            self.picked = order.get(from..=to).map(<[String]>::to_vec).unwrap_or_default();
            return;
        }
        if command {
            match self.picked.iter().position(|s| s == key) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(key.to_string()),
            }
        } else {
            self.picked = vec![key.to_string()];
        }
        self.anchor = Some(key.to_string());
    }

    /// Every filter back to showing all rows: the shelf, type, formats, vendor and the search.
    pub fn reset_filters(&mut self) {
        self.shelf = Shelf::All;
        self.types = TypeFilter::All;
        self.formats.clear();
        self.vendor = None;
        self.search.clear();
    }

    /// Each column's width: as dragged where it was, else the width at first.
    fn column_widths(&self) -> [f32; COLUMN_COUNT] {
        let mut widths = WIDTHS;
        for (index, width) in widths.iter_mut().enumerate() {
            if let Some(&kept) = self.widths.get(index)
                && kept > 0
            {
                *width = (kept as f32).max(MIN_WIDTHS.get(index).copied().unwrap_or(24.0));
            }
        }
        widths
    }

    /// Whether a column is shown: every one but those the columns menu hid.
    pub fn shows(&self, column: usize) -> bool {
        let name = COLUMNS.get(column).copied().unwrap_or_default();
        !HIDEABLE.contains(&column) || !self.hidden.iter().any(|h| h.eq_ignore_ascii_case(name))
    }

    fn set_shown(&mut self, column: usize, on: bool) {
        let Some(name) = COLUMNS.get(column).copied() else { return };
        self.hidden.retain(|h| !h.eq_ignore_ascii_case(name));
        if !on && HIDEABLE.contains(&column) {
            self.hidden.push(name.to_string());
        }
    }

    /// Each column's width as laid out: a hidden one has none.
    fn shown_widths(&self) -> [f32; COLUMN_COUNT] {
        let mut widths = self.column_widths();
        for (index, width) in widths.iter_mut().enumerate() {
            if !self.shows(index) {
                *width = 0.0;
            }
        }
        widths
    }

    /// How wide the table is laid out in `room`: the room, or more where the shown columns and a
    /// file column of [`FILE_WANT`] need more, which the table then scrolls sideways for.
    fn content_width(&self, room: f32, actions: bool) -> f32 {
        let mut widths = self.shown_widths();
        if !actions {
            widths[ACTION] = 0.0;
        }
        let others: f32 = widths.iter().enumerate().filter(|(i, _)| *i != FILE).map(|(_, w)| w).sum();
        let file = if self.shows(FILE) { FILE_WANT } else { 0.0 };
        room.max(others + file)
    }

    fn set_width(&mut self, column: usize, width: f32) {
        if self.widths.len() < COLUMN_COUNT {
            let current = self.column_widths();
            self.widths = current.iter().map(|w| w.round() as u32).collect();
        }
        let least = MIN_WIDTHS.get(column).copied().unwrap_or(24.0);
        if let Some(kept) = self.widths.get_mut(column) {
            *kept = width.max(least).round() as u32;
        }
    }
}

/// The key a file's row is picked by: never a plug-in's spec, which starts with its format.
fn file_key(path: &str) -> String {
    format!("file:{path}")
}

/// The rows of `keys` a right-click on the row of `key` acts on: the picked ones the list shows
/// (`order`), when that row is one of them, in the order they were picked; else that row alone.
/// A picked row the search hides, or a rescan removed, is left alone.
fn batch(picked: &[String], key: &str, order: &[String]) -> Vec<String> {
    if picked.iter().any(|s| s == key) {
        let shown: std::collections::HashSet<&str> = order.iter().map(String::as_str).collect();
        picked.iter().filter(|s| shown.contains(s.as_str())).cloned().collect()
    } else {
        vec![key.to_string()]
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

/// A row's state, which its status column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Disabled,
    Failed,
    WrongArchitecture,
}

impl State {
    fn word(self) -> &'static str {
        match self {
            State::Ready => "Ready",
            State::Disabled => "Disabled",
            State::Failed => "Failed",
            State::WrongArchitecture => "Wrong architecture",
        }
    }

    /// The mark's and the word's colour, from the theme.
    fn inks(self) -> (Color32, Color32) {
        match self {
            State::Ready => (theme::meter_normal(), theme::text_secondary()),
            State::Disabled => (theme::text_hint(), theme::text_hint()),
            State::Failed => (theme::recording(), theme::recording()),
            State::WrongArchitecture => (theme::meter_hot(), theme::meter_hot()),
        }
    }

    fn warns(self) -> bool {
        matches!(self, State::Failed | State::WrongArchitecture)
    }
}

/// The texts of a row's columns, what it stands for and the key it is picked by.
struct Line {
    kind: Kind,
    state: State,
    key: String,
    /// The file's path.
    file: String,
    cells: [String; COLUMN_COUNT],
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// The folder a file is in, for a label that tells two files of the same name apart.
fn folder_of(path: &str) -> String {
    std::path::Path::new(path).parent().map(|p| p.display().to_string()).unwrap_or_default()
}

/// The format a file is for, from its extension: a `.vst3` or `.clap` bundle, else a VST2 library.
fn file_format(path: &str) -> String {
    let extension =
        std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "vst3" | "clap" => extension,
        _ => "vst2".into(),
    }
}

/// Every row, before the sidebar's filters and the search.
fn all_lines(plugins: &PluginsView) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    for (index, p) in plugins.found.iter().enumerate() {
        let state = if p.enabled { State::Ready } else { State::Disabled };
        let cells = [
            String::new(),
            p.name.clone(),
            p.vendor.clone(),
            capitalised(&p.kind),
            p.format.to_ascii_uppercase(),
            p.version.clone(),
            state.word().into(),
            p.file.clone(),
            String::new(),
        ];
        lines.push(Line { kind: Kind::Plugin(index), state, key: p.spec.clone(), file: p.file.clone(), cells });
    }
    let others = plugins.failed.iter().enumerate().map(|(i, f)| (Kind::Failed(i), f, State::Failed)).chain(
        plugins.incompatible.iter().enumerate().map(|(i, f)| (Kind::Incompatible(i), f, State::WrongArchitecture)),
    );
    for (kind, (file, why), state) in others {
        let cells = [
            String::new(),
            file_name(file),
            String::new(),
            String::new(),
            file_format(file).to_ascii_uppercase(),
            String::new(),
            state.word().into(),
            why.clone(),
            String::new(),
        ];
        lines.push(Line { kind, state, key: file_key(file), file: file.clone(), cells });
    }
    lines
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// Whether the sidebar's shelf, type, formats and vendor keep `line`.
fn kept(line: &Line, plugins: &PluginsView, state: &ManagerState) -> bool {
    let plugin = match line.kind {
        Kind::Plugin(i) => plugins.found.get(i),
        _ => None,
    };
    let shelf = match state.shelf {
        Shelf::All => true,
        Shelf::Enabled => line.state == State::Ready,
        Shelf::Disabled => line.state == State::Disabled,
        Shelf::Owned => plugin.is_some_and(|p| p.owned),
        Shelf::Attention => line.state.warns(),
        Shelf::Failed => line.state == State::Failed,
        Shelf::Incompatible => line.state == State::WrongArchitecture,
    };
    let kind = match state.types {
        TypeFilter::All => true,
        TypeFilter::Effects => plugin.is_some_and(|p| p.kind.eq_ignore_ascii_case("effect")),
        TypeFilter::Instruments => plugin.is_some_and(|p| p.kind.eq_ignore_ascii_case("instrument")),
    };
    let format = state.formats.is_empty() || {
        let format = plugin.map_or_else(|| file_format(&line.file), |p| p.format.to_ascii_lowercase());
        state.formats.contains(&format)
    };
    let vendor = state.vendor.as_ref().is_none_or(|v| plugin.is_some_and(|p| &p.vendor == v));
    shelf && kind && format && vendor
}

/// Every row the filters and the search keep, sorted.
fn lines(plugins: &PluginsView, state: &ManagerState) -> Vec<Line> {
    let mut lines = all_lines(plugins);
    let words: Vec<String> = state.search.split_whitespace().map(str::to_lowercase).collect();
    lines.retain(|line| {
        let text = format!("{} {} {} {}", line.cells[1], line.cells[2], line.cells[FILE], line.file).to_lowercase();
        words.iter().all(|w| text.contains(w.as_str())) && kept(line, plugins, state)
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

/// How many rows each shelf of the sidebar holds.
#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    plugins: usize,
    enabled: usize,
    disabled: usize,
    owned: usize,
    failed: usize,
    incompatible: usize,
}

impl Counts {
    fn of(plugins: &PluginsView) -> Self {
        let enabled = plugins.found.iter().filter(|p| p.enabled).count();
        Self {
            plugins: plugins.found.len(),
            enabled,
            disabled: plugins.found.len() - enabled,
            owned: plugins.found.iter().filter(|p| p.owned).count(),
            failed: plugins.failed.len(),
            incompatible: plugins.incompatible.len(),
        }
    }

    fn attention(&self) -> usize {
        self.failed + self.incompatible
    }

    /// The notice over the table: "6 files failed to scan · 6 use an incompatible architecture".
    fn notice(&self) -> Option<String> {
        let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_string() } else { many.to_string() };
        let mut parts = Vec::new();
        if self.failed > 0 {
            parts.push(format!("{} {} to scan", self.failed, plural(self.failed, "file failed", "files failed")));
        }
        if self.incompatible > 0 {
            let verb = plural(self.incompatible, "uses", "use");
            let what = if self.failed > 0 { String::new() } else { plural(self.incompatible, " file", " files") };
            parts.push(format!("{}{what} {verb} an incompatible architecture", self.incompatible));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// The vendors of the plug-ins found, each once, in natural order.
fn vendors(plugins: &PluginsView) -> Vec<String> {
    let mut vendors: Vec<String> =
        plugins.found.iter().map(|p| p.vendor.clone()).filter(|v| !v.trim().is_empty()).collect();
    vendors.sort_by(|a, b| suhina_library::natural_cmp(a, b));
    vendors.dedup();
    vendors
}

/// The left and right edge of each column across `area`, with `widths` as kept (0 for a hidden
/// one); the file, where `file` shows it, takes what the others leave, at least its narrowest, and
/// the action column is left out (no width) when no file row is shown.
fn spans(area: Rect, widths: &[f32; COLUMN_COUNT], actions: bool, file: bool) -> [(f32, f32); COLUMN_COUNT] {
    let mut widths = *widths;
    if !actions {
        widths[ACTION] = 0.0;
    }
    let others: f32 = widths.iter().enumerate().filter(|(i, _)| *i != FILE).map(|(_, w)| w).sum();
    widths[FILE] = if file { (area.width() - others).max(MIN_WIDTHS[FILE]) } else { 0.0 };
    let mut x = area.left();
    let mut spans = [(0.0, 0.0); COLUMN_COUNT];
    for (span, width) in spans.iter_mut().zip(widths) {
        *span = (x, x + width);
        x += width;
    }
    spans
}

/// Whether a column's right edge is dragged: the ones between the name and the status. The
/// checkbox and the action keep their width, and the file fills what is left.
fn resizable(column: usize) -> bool {
    (1..FILE).contains(&column)
}

/// The width that fits a column's heading and every one of `lines`' text in it.
fn fit_width(ui: &Ui, column: usize, lines: &[Line]) -> f32 {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let heading = COLUMNS.get(column).copied().unwrap_or_default();
    // The heading leaves room for the sort mark, the status for its mark.
    let mut widest = widgets::text_width(ui, heading, small.clone()) + 20.0;
    for line in lines {
        if let Some(text) = line.cells.get(column) {
            let mark = if column == 6 { 14.0 } else { 0.0 };
            widest = widest.max(widgets::text_width(ui, text, small.clone()) + 10.0 + mark);
        }
    }
    widest.clamp(MIN_WIDTHS.get(column).copied().unwrap_or(24.0), FIT_MAX)
}

/// What the manager asks for this frame.
#[derive(Debug, Default)]
pub struct ManagerOut {
    pub request: Option<PluginRequest>,
    /// Close (or Escape) was clicked: Settings shows again.
    pub close: bool,
    /// The column widths changed (a drag let go, a double-click): the UI settings keep them.
    pub widths: Option<Vec<u32>>,
    /// The columns hidden changed: the UI settings keep them.
    pub hidden: Option<Vec<String>>,
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

/// A warning mark: a triangle in `color` around `center`.
fn warn_mark(painter: &egui::Painter, center: Pos2, size: f32, color: Color32) {
    let h = size * 0.5;
    painter.add(egui::Shape::convex_polygon(
        vec![center + Vec2::new(0.0, -h), center + Vec2::new(h, h * 0.85), center + Vec2::new(-h, h * 0.85)],
        color,
        egui::Stroke::NONE,
    ));
    let ink = theme::window_background();
    painter.line_segment(
        [center + Vec2::new(0.0, -h * 0.35), center + Vec2::new(0.0, h * 0.25)],
        egui::Stroke::new(1.2, ink),
    );
    painter.circle_filled(center + Vec2::new(0.0, h * 0.55), 0.7, ink);
}

/// The mark of a plug-in marked as owned: a small ring in the accent with a dot in it.
fn owned_mark(painter: &egui::Painter, center: Pos2) {
    painter.circle_stroke(center, 4.0, egui::Stroke::new(1.3, theme::accent()));
    painter.circle_filled(center, 1.8, theme::accent());
}

/// The mark of a row's state: a dot, or the warning mark.
fn state_mark(painter: &egui::Painter, center: Pos2, state: State) {
    let (mark, _) = state.inks();
    if state.warns() {
        warn_mark(painter, center, 10.0, mark);
    } else {
        painter.circle_filled(center, 3.5, mark);
    }
}

/// A clickable line of the sidebar: lit while `on`, with a count on the right and, where `warn`,
/// the warning mark before its text. True when clicked.
#[allow(clippy::too_many_arguments)]
fn side_item(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: &str,
    text: &str,
    count: Option<usize>,
    on: bool,
    warn: bool,
) -> bool {
    let label = match count {
        Some(count) => format!("Show {text} ({count})"),
        None => format!("Show {text}"),
    };
    let response = widgets::button_area(ui, rect, Id::unique(("manager-side", id)), label);
    let hovered = widgets::hover(ui, &response);
    if on {
        painter.rect_filled(rect, 3.0, theme::control_active());
    } else if hovered > 0.0 {
        painter.rect_filled(rect, 3.0, theme::panel_hover().gamma_multiply(hovered));
    }
    let small = theme::ui_font(theme::TEXT_SMALL);
    let mut left = rect.left() + 8.0;
    if warn {
        warn_mark(painter, Pos2::new(left + 5.0, rect.center().y), 10.0, theme::meter_hot());
        left += 16.0;
    }
    let ink = if on { theme::text_primary() } else { theme::text_secondary() };
    let words = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(rect.right() - 36.0, rect.bottom()));
    widgets::text_in(painter, words, Align2::LEFT_CENTER, text, small.clone(), ink);
    if let Some(count) = count {
        let number = Rect::from_min_max(
            Pos2::new(rect.right() - 44.0, rect.top()),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        );
        widgets::text_in(painter, number, Align2::RIGHT_CENTER, &count.to_string(), small, theme::text_hint());
    }
    response.clicked()
}

/// A section heading of the sidebar or the panel: small, in capitals, quiet.
fn heading(painter: &egui::Painter, rect: Rect, text: &str) {
    let tiny = theme::ui_font(theme::TEXT_TINY);
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, &text.to_uppercase(), tiny, theme::text_hint());
}

/// A link: text in the accent, underlined, with `label` for readers and tests. True when clicked.
fn link(ui: &Ui, painter: &egui::Painter, rect: Rect, id: Id, text: &str, label: &str, align: Align2) -> bool {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let width = widgets::text_width(ui, text, small.clone()).min(rect.width());
    let at = align.align_size_within_rect(Vec2::new(width, rect.height()), rect);
    let response = widgets::button_area(ui, at, id, label).on_hover_cursor(egui::CursorIcon::PointingHand);
    let ink = motion::mix(theme::accent(), theme::text_primary(), widgets::hover(ui, &response));
    widgets::text_in(painter, at, Align2::LEFT_CENTER, text, small, ink);
    let y = at.center().y + 7.0;
    painter.hline(at.x_range(), y, egui::Stroke::new(1.0, ink));
    response.clicked()
}

/// The parts of the manager's box, each `None` where the dialog is too narrow for it.
struct Parts {
    top: Rect,
    sidebar: Option<Rect>,
    middle: Rect,
    detail: Option<Rect>,
    status: Rect,
}

fn parts(inner: Rect) -> Parts {
    let top = Rect::from_min_size(inner.min, Vec2::new(inner.width(), TOP));
    let status = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - STATUS), inner.max);
    let body = Rect::from_min_max(Pos2::new(inner.left(), top.bottom()), Pos2::new(inner.right(), status.top()));
    let detail_fits = body.width() >= SIDEBAR + DETAIL + TABLE_MIN;
    let sidebar_fits = body.width() >= SIDEBAR + TABLE_MIN;
    let sidebar = sidebar_fits.then(|| Rect::from_min_max(body.min, Pos2::new(body.left() + SIDEBAR, body.bottom())));
    let detail = detail_fits.then(|| Rect::from_min_max(Pos2::new(body.right() - DETAIL, body.top()), body.max));
    let middle = Rect::from_min_max(
        Pos2::new(sidebar.map_or(body.left(), |s| s.right()), body.top()),
        Pos2::new(detail.map_or(body.right(), |d| d.left()), body.bottom()),
    );
    Parts { top, sidebar, middle, detail, status }
}

/// Draws the manager over `area`.
pub fn show(ui: &mut Ui, area: Rect, plugins: &mut PluginsView) -> ManagerOut {
    // `plugin-manager` styles it; its buttons and fields take the sheet's `button` and `text-input`.
    let page = crate::kit::element::PLUGIN_MANAGER
        .native(crate::kit::element::Native {
            radius: 0.0,
            fill: crate::kit::style_class::surface_or(theme::window_background()),
        })
        .enter();
    let mut styled_ui = page.ui(ui);
    let ui = &mut *styled_ui;
    let mut out = ManagerOut::default();
    let painter = ui.painter_at(area);
    // What the page holds goes in its content box, inside its border and padding (#1336).
    let area = page.paint(&painter, area).content;
    let parts = parts(area);
    let line = theme::hairline(theme::border());
    painter.hline(parts.top.x_range(), parts.top.bottom() - 0.5, line);
    painter.hline(parts.status.x_range(), parts.status.top() + 0.5, line);
    if let Some(sidebar) = parts.sidebar {
        painter.vline(sidebar.right() - 0.5, sidebar.y_range(), line);
    }
    if let Some(detail) = parts.detail {
        painter.vline(detail.left() + 0.5, detail.y_range(), line);
    }
    let counts = Counts::of(plugins);

    top_bar(ui, &painter, parts.top, plugins, &counts, &mut out);
    if let Some(sidebar) = parts.sidebar {
        library(ui, &painter, sidebar.shrink(PAD * 1.5), plugins, &counts);
    }
    let lines = lines(plugins, &plugins.manager);
    if plugins.manager.locations {
        scan_locations(ui, &painter, parts.middle.shrink(PAD * 1.5), plugins, &mut out);
    } else {
        table(ui, &painter, parts.middle.shrink(PAD * 1.5), plugins, &counts, &lines, &mut out);
    }
    if let Some(detail) = parts.detail {
        selected(ui, &painter, detail.shrink2(Vec2::new(PAD * 2.5, PAD * 2.0)), plugins, &mut out);
    }
    status_bar(&painter, parts.status.shrink2(Vec2::new(PAD * 2.0, 0.0)), plugins, &lines);
    out
}

/// The title and the count on the left; Scan for new, Rescan all and Close on the right.
fn top_bar(ui: &Ui, painter: &egui::Painter, rect: Rect, plugins: &PluginsView, counts: &Counts, out: &mut ManagerOut) {
    let rect = rect.shrink2(Vec2::new(PAD * 2.0, 0.0));
    let title_font = theme::ui_font(theme::TEXT + 3.0);
    let title = "Plug-in manager";
    let title_width = widgets::text_width(ui, title, title_font.clone());
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, title, title_font, theme::text_primary());
    let small = theme::ui_font(theme::TEXT_SMALL);
    let count = format!("{} plug-ins", counts.plugins);
    let note = Rect::from_min_max(
        Pos2::new(rect.left() + title_width + 14.0, rect.top()),
        Pos2::new(rect.right() - 320.0, rect.bottom()),
    );
    widgets::text_in(painter, note, Align2::LEFT_CENTER, &count, small.clone(), theme::text_secondary());
    let mut x = rect.right();
    let busy = plugins.scanning;
    for (text, label, primary) in [
        ("Close", "Close the plug-in manager", false),
        ("Rescan all", "Rescan all plug-ins", false),
        ("Scan for new", "Scan for plug-ins", true),
    ] {
        let width = widgets::text_width(ui, text, small.clone()) + 28.0;
        x -= width;
        let button = Rect::from_min_size(Pos2::new(x, rect.center().y - 12.0), Vec2::new(width, 24.0));
        x -= 6.0;
        let id = Id::unique(("manager-top", text));
        if widgets::button_with_id(ui, painter, button, id, text, label, primary).clicked() {
            match text {
                "Close" => out.close = true,
                _ if busy => {}
                "Scan for new" => out.request = Some(PluginRequest::Scan { all: false }),
                _ => out.request = Some(PluginRequest::Scan { all: true }),
            }
        }
    }
}

/// The library sidebar: the shelves with their counts, the type, format and vendor filters, then
/// Scan locations and Reset filters at the bottom.
fn library(ui: &mut Ui, painter: &egui::Painter, rect: Rect, plugins: &mut PluginsView, counts: &Counts) {
    // The vendor list is made from every plug-in found, whatever the filters keep.
    let names = vendors(plugins);
    let folders = plugins.folders.iter().filter(|f| f.enabled).count();
    let state = &mut plugins.manager;
    let mut y = rect.top();
    let row = |y: &mut f32, height: f32| {
        let r = Rect::from_min_size(Pos2::new(rect.left(), *y), Vec2::new(rect.width(), height));
        *y += height;
        r
    };
    let title = row(&mut y, 24.0);
    widgets::text_in(
        painter,
        title,
        Align2::LEFT_CENTER,
        "Library",
        theme::ui_font(theme::TEXT + 1.0),
        theme::text_primary(),
    );
    y += 4.0;
    let shelves = [
        (Shelf::All, "All plug-ins", counts.plugins + counts.attention(), false),
        (Shelf::Enabled, "Enabled", counts.enabled, false),
        (Shelf::Disabled, "Disabled", counts.disabled, false),
        (Shelf::Owned, "Owned", counts.owned, false),
        (Shelf::Attention, "Needs attention", counts.attention(), counts.attention() > 0),
    ];
    for (shelf, text, count, warn) in shelves {
        if side_item(ui, painter, row(&mut y, ITEM), text, text, Some(count), state.shelf == shelf, warn) {
            state.shelf = shelf;
            state.locations = false;
        }
    }
    // The two kinds of file that need attention, under it.
    if counts.attention() > 0 {
        for (shelf, text, count) in
            [(Shelf::Failed, "Failed", counts.failed), (Shelf::Incompatible, "Other processor", counts.incompatible)]
        {
            let item = row(&mut y, ITEM);
            let item = Rect::from_min_max(Pos2::new(item.left() + 16.0, item.top()), item.max);
            if side_item(ui, painter, item, text, text, Some(count), state.shelf == shelf, false) {
                state.shelf = shelf;
                state.locations = false;
            }
        }
    }
    y += 8.0;
    painter.hline(rect.x_range(), y, theme::hairline(theme::border()));
    y += 8.0;
    heading(painter, row(&mut y, 18.0), "Type");
    for (types, text) in
        [(TypeFilter::All, "All types"), (TypeFilter::Effects, "Effects"), (TypeFilter::Instruments, "Instruments")]
    {
        if side_item(ui, painter, row(&mut y, ITEM), text, text, None, state.types == types, false) {
            state.types = types;
            state.locations = false;
        }
    }
    y += 8.0;
    painter.hline(rect.x_range(), y, theme::hairline(theme::border()));
    y += 8.0;
    heading(painter, row(&mut y, 18.0), "Format");
    for (format, text) in [("clap", "CLAP"), ("vst3", "VST3"), ("vst2", "VST2")] {
        let item = row(&mut y, ITEM);
        let mut on = state.formats.iter().any(|f| f == format);
        let check =
            Rect::from_min_size(Pos2::new(item.left() + 6.0, item.top()), Vec2::new(item.width() - 6.0, item.height()));
        // Laid out from the left of its line, as the sidebar's other items are, not centred in it.
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(check).layout(layout), |ui| {
                ui.add(egui::Checkbox::new(&mut on, text))
            })
            .inner;
        let label = format!("Show {text} plug-ins");
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &label));
        if response.changed() {
            state.formats.retain(|f| f != format);
            if on {
                state.formats.push(format.to_string());
            }
        }
    }
    y += 8.0;
    painter.hline(rect.x_range(), y, theme::hairline(theme::border()));
    y += 8.0;
    heading(painter, row(&mut y, 18.0), "Vendor");
    let vendor_row = row(&mut y, 24.0);
    let shown = state.vendor.clone().unwrap_or_else(|| "All vendors".into());
    ui.scope_builder(egui::UiBuilder::new().max_rect(vendor_row), |ui| {
        egui::ComboBox::from_id_salt("plugin-manager-vendor")
            .selected_text(shown)
            .width(vendor_row.width())
            .height(320.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.vendor, None, "All vendors");
                for name in names {
                    let value = Some(name.clone());
                    ui.selectable_value(&mut state.vendor, value, name);
                }
            })
    });

    // At the bottom: the folders scans look in (Settings › Plug-ins), and the filters undone.
    let reset = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 20.0), rect.max);
    let locations =
        Rect::from_min_max(Pos2::new(rect.left(), reset.top() - 34.0), Pos2::new(rect.right(), reset.top() - 6.0));
    if locations.top() > y + 8.0 {
        let id = Id::unique("manager-scan-locations");
        let label = "Scan locations: the folders scans look in";
        let lit = state.locations;
        let response = widgets::button_with_id(ui, painter, locations, id, "Scan locations", label, lit);
        let response = response.on_hover_text(format!("{folders} folders are scanned"));
        if response.clicked() {
            state.locations = !state.locations;
        }
        if link(
            ui,
            painter,
            reset,
            Id::unique("manager-reset"),
            "Reset filters",
            "Reset filters",
            Align2::CENTER_CENTER,
        ) {
            state.reset_filters();
        }
    }
}

/// The folders scans look in, in the middle in the table's place: each on or off, the added ones
/// removed, a folder added with the format it holds, as on the Settings page, which draws the same
/// list. Escape, or **Back to the plug-ins**, shows the table again.
fn scan_locations(ui: &mut Ui, painter: &egui::Painter, rect: Rect, plugins: &mut PluginsView, out: &mut ManagerOut) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let top = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 24.0));
    let title = theme::ui_font(theme::TEXT + 1.0);
    widgets::text_in(painter, top, Align2::LEFT_CENTER, "Scan locations", title, theme::text_primary());
    let back = Rect::from_min_max(Pos2::new(top.right() - 160.0, top.top()), top.max);
    let id = Id::unique("manager-back");
    if link(ui, painter, back, id, "Back to the plug-ins", "Back to the plug-ins", Align2::RIGHT_CENTER) {
        plugins.manager.locations = false;
    }
    let hint = Rect::from_min_size(Pos2::new(rect.left(), top.bottom() + 2.0), Vec2::new(rect.width(), 20.0));
    let words = "Scan for new looks in the folders ticked; a folder switched off leaves the list with the next scan.";
    widgets::text_in(painter, hint, Align2::LEFT_CENTER, words, small, theme::text_hint());
    let list = Rect::from_min_max(Pos2::new(rect.left(), hint.bottom() + 10.0), rect.max);
    let rows = plugins.folders.len() as f32;
    // The list's own height: its heading, a line per folder, the add row and the space after.
    let height = 18.0 + rows * 18.0 + 4.0 + 20.0 + 8.0;
    let mut request = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("plugin-manager-folders").auto_shrink([false, false]).show(ui, |ui| {
            let (page, _) = ui.allocate_exact_size(Vec2::new(list.width() - 12.0, height), Sense::hover());
            let page_painter = ui.painter_at(page.expand2(Vec2::new(0.0, 2.0)));
            let mut y = page.top();
            request = super::settings_view::plugin_folders(ui, &page_painter, page, &mut y, page.bottom(), plugins);
        });
    });
    if request.is_some() {
        out.request = request;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.ctx().egui_wants_keyboard_input() {
        plugins.manager.locations = false;
    }
}

/// The plug-ins among `keys` (files left out) split into those marked as owned and the others,
/// each in the order of `keys`.
fn split_by_owned(keys: &[String], plugins: &PluginsView) -> (Vec<String>, Vec<String>) {
    let mut owned = Vec::new();
    let mut others = Vec::new();
    for key in keys {
        if let Some(plugin) = plugins.found.iter().find(|p| &p.spec == key) {
            if plugin.owned { owned.push(key.clone()) } else { others.push(key.clone()) }
        }
    }
    (owned, others)
}

/// **Turn off the rest** of `vendor`: its plug-ins switched on and not marked as owned. Nothing
/// where none of the vendor's plug-ins is marked, so a vendor no one marked is never switched off
/// whole by mistake.
fn rest_of_vendor(vendor: &str, plugins: &PluginsView) -> Vec<String> {
    let theirs = || plugins.found.iter().filter(|p| !vendor.is_empty() && p.vendor == vendor);
    if !theirs().any(|p| p.owned) {
        return Vec::new();
    }
    theirs().filter(|p| p.enabled && !p.owned).map(|p| p.spec.clone()).collect()
}

/// The plug-ins among `keys` (files left out) split into those switched on and those off, each in
/// the order of `keys`.
fn split_by_state(keys: &[String], plugins: &PluginsView) -> (Vec<String>, Vec<String>) {
    let mut on = Vec::new();
    let mut off = Vec::new();
    for key in keys {
        if let Some(plugin) = plugins.found.iter().find(|p| &p.spec == key) {
            if plugin.enabled { on.push(key.clone()) } else { off.push(key.clone()) }
        }
    }
    (on, off)
}

/// The id of a file row's **Scan again**: from the file's whole path, so two files of the same
/// name in different folders get a button each (#1461). Not under the scroll area's id, which a
/// row's place in the list would change.
fn scan_again_id(file: &str) -> Id {
    Id::unique(("manager-scan-again", file))
}

/// A file row's **Scan again** label: the file's name and its folder, so a reader tells two
/// files of the same name apart.
fn scan_again_label(file: &str) -> String {
    format!("Scan again {} in {}", file_name(file), folder_of(file))
}

/// What a row's cells, menu and buttons asked for this frame.
#[derive(Default)]
struct Asked {
    toggled: Option<(String, bool)>,
    toggled_many: Option<(Vec<String>, bool)>,
    owned_many: Option<(Vec<String>, bool)>,
    pick: Option<(String, bool, bool)>,
    again: Option<Vec<String>>,
    show: Option<String>,
}

/// The search, the notice of files that need attention, and the table.
fn table(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    plugins: &mut PluginsView,
    counts: &Counts,
    lines: &[Line],
    out: &mut ManagerOut,
) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    // The search, and beside it **Columns**, the menu that shows or hides columns.
    let columns_width = 84.0;
    let search = Rect::from_min_size(rect.min, Vec2::new((rect.width() - columns_width - 8.0).max(80.0), 24.0));
    let chooser =
        Rect::from_min_max(Pos2::new(search.right() + 8.0, search.top()), Pos2::new(rect.right(), search.bottom()));
    let id = Id::unique("manager-columns");
    let response = widgets::button_with_id(ui, painter, chooser, id, "Columns", "Choose the columns shown", false);
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| columns_menu(ui, &mut plugins.manager, out));
    let edit = ui.put(
        search,
        egui::TextEdit::singleline(&mut plugins.manager.search)
            .frame(widgets::input_frame())
            .hint_text("Search by name, vendor or file")
            .font(small.clone()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Find a plug-in"));
    widgets::input_line(ui, &edit);
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !edit.has_focus() {
        out.close = true;
    }
    let mut y = search.bottom() + 8.0;
    if let Some(text) = counts.notice() {
        let notice = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), 28.0));
        let warn = theme::meter_hot();
        painter.rect_filled(notice, 3.0, warn.gamma_multiply(0.12));
        painter.rect_stroke(notice, 3.0, theme::hairline(warn.gamma_multiply(0.6)), egui::StrokeKind::Inside);
        warn_mark(painter, Pos2::new(notice.left() + 14.0, notice.center().y), 11.0, warn);
        let words = Rect::from_min_max(
            Pos2::new(notice.left() + 28.0, notice.top()),
            Pos2::new(notice.right() - 110.0, notice.bottom()),
        );
        widgets::text_in(painter, words, Align2::LEFT_CENTER, &text, small.clone(), warn);
        let review = Rect::from_min_max(
            Pos2::new(notice.right() - 106.0, notice.top()),
            Pos2::new(notice.right() - 10.0, notice.bottom()),
        );
        let id = Id::unique("manager-review");
        if link(ui, painter, review, id, "Review issues", "Review issues", Align2::RIGHT_CENTER) {
            plugins.manager.shelf = Shelf::Attention;
        }
        y = notice.bottom() + 8.0;
    }

    // The headings and the rows scroll sideways together when the columns are wider than the
    // room; the rows also scroll down, under headings that stay.
    let order: Vec<String> = lines.iter().map(|l| l.key.clone()).collect();
    let table = Rect::from_min_max(Pos2::new(rect.left(), y), rect.max);
    if lines.is_empty() {
        let nothing = plugins.found.is_empty() && plugins.failed.is_empty() && plugins.incompatible.is_empty();
        let text =
            if nothing { "No plug-ins found yet: Scan for new looks for them" } else { "Nothing matches the filters" };
        let header = Rect::from_min_size(table.min, Vec2::new(table.width(), ROW + 4.0));
        headings(ui, painter, header, plugins, lines, &order, out);
        let below = Rect::from_min_max(Pos2::new(table.left(), header.bottom()), table.max);
        widgets::text_in(painter, below.shrink(PAD * 2.0), Align2::CENTER_TOP, text, small, theme::text_hint());
        return;
    }
    let actions = lines.iter().any(|l| !matches!(l.kind, Kind::Plugin(_)));
    let total = plugins.manager.content_width(table.width(), actions);
    let mut asked = Asked::default();
    ui.scope_builder(egui::UiBuilder::new().max_rect(table), |ui| {
        egui::ScrollArea::horizontal().id_salt("plugin-manager-across").auto_shrink([false, false]).show(ui, |ui| {
            let (header, _) = ui.allocate_exact_size(Vec2::new(total, ROW + 4.0), Sense::hover());
            let header_painter = ui.painter_at(header);
            headings(ui, &header_painter, header, plugins, lines, &order, out);
            let widths = plugins.manager.shown_widths();
            let file = plugins.manager.shows(FILE);
            let picked = plugins.manager.picked.clone();
            let anchor = plugins.manager.anchor.clone();
            let scanning = plugins.scanning;
            let shown = Shown {
                picked: &picked,
                anchor: anchor.as_deref(),
                order: &order,
                widths: &widths,
                actions,
                file,
                scanning,
            };
            let view: &PluginsView = plugins;
            egui::ScrollArea::vertical().id_salt("plugin-manager-down").auto_shrink([false, false]).show_rows(
                ui,
                ROW,
                lines.len(),
                |ui, range| {
                    for line in lines.get(range).unwrap_or_default() {
                        row(ui, line, total, view, &shown, &mut asked);
                    }
                },
            );
        });
    });
    if let Some((key, command, shift)) = asked.pick {
        plugins.manager.pick(&key, command, shift, &order);
    }
    if let Some((specs, enabled)) = asked.toggled_many {
        out.request = Some(PluginRequest::SetEnabledMany(specs, enabled));
    } else if let Some((specs, owned)) = asked.owned_many {
        out.request = Some(PluginRequest::SetOwnedMany(specs, owned));
    } else if let Some((spec, enabled)) = asked.toggled {
        out.request = Some(PluginRequest::SetEnabled(spec, enabled));
    } else if let Some(files) = asked.again {
        out.request = Some(PluginRequest::ScanFiles(files));
    } else if let Some(file) = asked.show {
        out.request = Some(PluginRequest::ShowFile(file));
    }
}

/// The columns that can be hidden, by index: all but the checkbox, the name and the action.
const HIDEABLE: [usize; 6] = [2, 3, 4, 5, 6, FILE];

/// The menu that shows or hides columns: on a right-click on the headings, and under **Columns**.
fn columns_menu(ui: &mut Ui, state: &mut ManagerState, out: &mut ManagerOut) {
    for index in HIDEABLE {
        let name = COLUMNS[index];
        let mut on = state.shows(index);
        if ui.checkbox(&mut on, name).changed() {
            state.set_shown(index, on);
            out.hidden = Some(state.hidden.clone());
        }
    }
    ui.separator();
    if ui.button("Show every column").clicked() {
        state.hidden.clear();
        out.hidden = Some(Vec::new());
        ui.close();
    }
}

/// The headings across `header`: the checkbox that picks every row shown, a button per column
/// that sorts by it, and the edges dragged to resize the columns. A right-click on them shows
/// or hides columns.
fn headings(
    ui: &mut Ui,
    painter: &egui::Painter,
    header: Rect,
    plugins: &mut PluginsView,
    lines: &[Line],
    order: &[String],
    out: &mut ManagerOut,
) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let actions = lines.iter().any(|l| !matches!(l.kind, Kind::Plugin(_)));
    let raw = plugins.manager.column_widths();
    let widths = plugins.manager.shown_widths();
    let columns = spans(header, &widths, actions, plugins.manager.shows(FILE));
    painter.hline(header.x_range(), header.bottom() - 0.5, theme::hairline(theme::border()));
    // Beneath the headings: a right-click between them opens the columns menu too.
    let under = ui.interact(header, Id::unique("manager-headings"), Sense::click());
    // Stays open while columns are ticked: a click inside would close a plain context menu.
    egui::Popup::context_menu(&under)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| columns_menu(ui, &mut plugins.manager, out));
    {
        let (left, right) = columns[0];
        let state = &mut plugins.manager;
        let all = !order.is_empty() && order.iter().all(|k| state.picked.contains(k));
        let mut every = all;
        let check = Rect::from_center_size(Pos2::new((left + right) / 2.0, header.center().y), Vec2::splat(16.0));
        let response = ui.put(check, egui::Checkbox::without_text(&mut every));
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, all, "Pick every row shown"));
        if response.changed() {
            if every {
                state.picked = order.to_vec();
                state.anchor = order.first().cloned();
            } else {
                state.picked.clear();
            }
        }
    }
    for (index, (name, (left, right))) in COLUMNS.iter().zip(columns).enumerate().skip(1) {
        if name.is_empty() || right - left < 8.0 {
            continue;
        }
        let cell = Rect::from_min_max(Pos2::new(left, header.top()), Pos2::new(right, header.bottom()));
        let id = Id::unique(("manager-column", index));
        let response = widgets::button_area(ui, cell, id, format!("Sort the plug-ins by {name}"));
        if response.clicked() {
            let (sorted, descending) = plugins.manager.sort;
            plugins.manager.sort = (index, if sorted == index { !descending } else { false });
        }
        // Stays open while columns are ticked: a click inside would close a plain context menu.
        egui::Popup::context_menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| columns_menu(ui, &mut plugins.manager, out));
        let (sorted, descending) = plugins.manager.sort;
        let label = if sorted == index {
            format!("{name} {}", if descending { "▼" } else { "▲" })
        } else {
            (*name).to_string()
        };
        let color = if sorted == index { theme::text_primary() } else { theme::text_secondary() };
        let hovered = widgets::hover(ui, &response);
        if hovered > 0.0 {
            painter.rect_filled(cell, 0.0, theme::panel_hover().gamma_multiply(hovered));
        }
        let words = cell.shrink2(Vec2::new(4.0, 0.0));
        widgets::text_in(painter, words, Align2::LEFT_CENTER, &label, small.clone(), color);
    }
    // Drawn after the headings, so the edge, not the heading under it, takes the pointer.
    let edges: Vec<usize> = (1..COLUMN_COUNT).filter(|i| resizable(*i) && plugins.manager.shows(*i)).collect();
    for index in edges {
        let (_, right) = columns[index];
        let edge = Rect::from_min_max(Pos2::new(right - 4.0, header.top()), Pos2::new(right + 4.0, header.bottom()));
        let response = ui
            .interact(edge, Id::unique(("manager-edge", index)), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeColumn);
        let name = COLUMNS[index];
        let label = format!("Resize the {name} column");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, &label));
        let state = &mut plugins.manager;
        if response.double_clicked() {
            state.set_width(index, fit_width(ui, index, lines));
            out.widths = Some(state.widths.clone());
        } else if response.dragged() {
            state.set_width(index, raw[index] + response.drag_delta().x);
        }
        if response.drag_stopped() {
            out.widths = Some(state.widths.clone());
        }
        let lit = response.hovered() || response.dragged();
        let ink = if lit { theme::accent() } else { theme::border() };
        let span = egui::Rangef::new(header.top() + 5.0, header.bottom() - 5.0);
        painter.vline(right - 0.5, span, egui::Stroke::new(if lit { 2.0 } else { 1.0 }, ink));
    }
}

/// What every row of a frame shares: the rows picked, the one picked last, the order shown, the
/// column widths, whether the action column is there, and whether a scan runs.
struct Shown<'a> {
    picked: &'a [String],
    anchor: Option<&'a str>,
    order: &'a [String],
    widths: &'a [f32; COLUMN_COUNT],
    actions: bool,
    /// The file column is shown, and fills the width the others leave.
    file: bool,
    scanning: bool,
}

/// One row of the table: its fill, its cells with their full text on hover, its checkbox or its
/// **Scan again**, its click and its right-click menu.
fn row(ui: &mut Ui, line: &Line, width: f32, plugins: &PluginsView, shown: &Shown<'_>, asked: &mut Asked) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let scanning = shown.scanning;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW), Sense::click());
    let columns = spans(rect, shown.widths, shown.actions, shown.file);
    let painter = ui.painter_at(rect);
    let lit = shown.picked.contains(&line.key);
    if lit {
        painter.rect_filled(rect, 0.0, theme::control_active());
    } else if ui.rect_contains_pointer(rect) {
        painter.rect_filled(rect, 0.0, theme::panel_hover());
    }
    if shown.anchor == Some(line.key.as_str()) {
        let strip = Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height()));
        painter.rect_filled(strip, 0.0, theme::accent());
    }
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border().gamma_multiply(0.5)));
    // A click picks the row (Ctrl adds, Shift takes the run); the right-click menu acts on every
    // picked row of its kind (#725).
    if response.clicked() {
        let (command, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
        asked.pick = Some((line.key.clone(), command, shift));
    }
    if response.secondary_clicked() && !lit {
        asked.pick = Some((line.key.clone(), false, false));
    }
    let plugin = match line.kind {
        Kind::Plugin(i) => plugins.found.get(i),
        _ => None,
    };
    // Worked out only while the menu is open, not for every row every frame.
    response.context_menu(|ui| {
        let these = batch(shown.picked, &line.key, shown.order);
        if plugin.is_some() {
            // Only what a switch would change: Turn off for the ones on, Turn on for the ones off,
            // each counting those it acts on, and neither where there are none.
            let (on, off) = split_by_state(&these, plugins);
            for (specs, enabled, verb) in [(on, false, "Turn off"), (off, true, "Turn on")] {
                if specs.is_empty() {
                    continue;
                }
                let what = if specs.len() > 1 { format!("{} plug-ins", specs.len()) } else { "the plug-in".into() };
                if ui.add_enabled(!scanning, egui::Button::new(format!("{verb} {what}"))).clicked() {
                    asked.toggled_many = Some((specs, enabled));
                    ui.close();
                }
            }
            ui.separator();
            // Marked as owned: only the mark that changes something, as for the switch.
            let (owned, others) = split_by_owned(&these, plugins);
            for (specs, mark, verb) in [(others, true, "Mark"), (owned, false, "Unmark")] {
                if specs.is_empty() {
                    continue;
                }
                let what = if specs.len() > 1 { format!("{} plug-ins", specs.len()) } else { "the plug-in".into() };
                let text = format!("{verb} {what} as owned");
                if ui.add_enabled(!scanning, egui::Button::new(text)).clicked() {
                    asked.owned_many = Some((specs, mark));
                    ui.close();
                }
            }
            if let Some(vendor) = plugin.map(|p| p.vendor.clone()) {
                let rest = rest_of_vendor(&vendor, plugins);
                if !rest.is_empty() {
                    let text = format!("Turn off the rest of {vendor} ({})", rest.len());
                    let tip = "Every one of this vendor's plug-ins that is not marked as owned is switched off";
                    if ui.add_enabled(!scanning, egui::Button::new(text)).on_hover_text(tip).clicked() {
                        asked.toggled_many = Some((rest, false));
                        ui.close();
                    }
                }
            }
        } else {
            let files: Vec<String> = these.iter().filter_map(|k| k.strip_prefix("file:")).map(str::to_string).collect();
            let what = if files.len() > 1 { format!("Scan {} files again", files.len()) } else { "Scan again".into() };
            if ui.add_enabled(!scanning, egui::Button::new(what)).clicked() {
                asked.again = Some(files);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(show_in_folder_label()).clicked() {
            asked.show = Some(line.file.clone());
            ui.close();
        }
    });
    let disabled = line.state == State::Disabled;
    for (index, ((left, right), text)) in columns.iter().zip(&line.cells).enumerate().take(ACTION).skip(1) {
        let mut cell = Rect::from_min_max(Pos2::new(left + 4.0, rect.top()), Pos2::new(right - 4.0, rect.bottom()));
        if cell.width() <= 0.0 || text.is_empty() {
            continue;
        }
        let color = match (index, line.kind) {
            _ if disabled => theme::text_hint(),
            (1, Kind::Plugin(_)) => theme::text_value(),
            (6, _) => line.state.inks().1,
            (FILE, Kind::Failed(_)) => theme::recording(),
            (FILE, Kind::Incompatible(_)) => theme::meter_hot(),
            (1, _) => theme::text_primary(),
            (FILE, _) => theme::text_hint(),
            _ => theme::text_secondary(),
        };
        if index == 6 {
            state_mark(&painter, Pos2::new(cell.left() + 5.0, rect.center().y), line.state);
            cell.min.x += 14.0;
        }
        if index == 1 && plugin.is_some_and(|p| p.owned) {
            owned_mark(&painter, Pos2::new(cell.left() + 4.0, rect.center().y));
            cell.min.x += 13.0;
        }
        let galley = widgets::fitted(&painter, text, small.clone(), color, cell.width());
        let elided = galley.elided;
        widgets::paint_text(&painter, Pos2::new(cell.left(), rect.center().y - galley.size().y / 2.0), galley, color);
        // The full text on hover where the cell cuts it, and a file's whole path on its name.
        let full = if index == 1 && plugin.is_none() {
            Some(line.file.clone())
        } else if elided {
            Some(text.clone())
        } else {
            None
        };
        if let Some(full) = full
            && ui.rect_contains_pointer(cell)
        {
            let id = Id::unique(("manager-cell", &line.key, index));
            let _tip = ui.interact(cell, id, Sense::hover()).on_hover_text(full);
        }
    }
    let (on_left, on_right) = columns[0];
    let (action_left, action_right) = columns[ACTION];
    match (line.kind, plugin) {
        (Kind::Plugin(_), Some(plugin)) => {
            let mut enabled = plugin.enabled;
            let center = Pos2::new((on_left + on_right) / 2.0, rect.center().y);
            let check = Rect::from_center_size(center, Vec2::splat(16.0));
            // Off while a scan runs: its result replaces the library, switches too.
            let response = ui
                .put(check, |ui: &mut egui::Ui| ui.add_enabled(!scanning, egui::Checkbox::without_text(&mut enabled)));
            let label = format!("Use {}", plugin.name);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, plugin.enabled, &label));
            if response.changed() {
                asked.toggled = Some((plugin.spec.clone(), enabled));
            }
        }
        (Kind::Failed(_) | Kind::Incompatible(_), _) if action_right - action_left > 8.0 && !scanning => {
            let button = Rect::from_min_max(
                Pos2::new(action_left + 4.0, rect.top() + 2.0),
                Pos2::new(action_right - 2.0, rect.bottom() - 2.0),
            );
            let id = scan_again_id(&line.file);
            let label = scan_again_label(&line.file);
            if widgets::button_with_id(ui, &painter, button, id, "Scan again", &label, false).clicked() {
                asked.again = Some(vec![line.file.clone()]);
            }
        }
        _ => {}
    }
}

/// An on/off switch, in the accent while on. `label` names it for readers and tests.
fn switch(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    on: bool,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label));
    let t = motion::toward(ui.ctx(), id.with("on"), on, motion::HOVER);
    let fill = motion::mix(theme::control_background(), theme::accent(), t);
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, radius, if enabled { fill } else { fill.gamma_multiply(0.5) });
    painter.rect_stroke(rect, radius, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let x = rect.left() + radius + (rect.width() - 2.0 * radius) * t;
    let knob = motion::mix(theme::text_secondary(), theme::text_inverse(), t);
    painter.circle_filled(Pos2::new(x, rect.center().y), radius - 3.0, knob);
    response
}

/// A tag: a word in a thin frame. Returns its right edge.
fn tag(ui: &Ui, painter: &egui::Painter, at: Pos2, text: &str) -> f32 {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let width = widgets::text_width(ui, text, small.clone()) + 16.0;
    let rect = Rect::from_min_size(at, Vec2::new(width, 20.0));
    painter.rect_stroke(rect, 3.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, small, theme::text_secondary());
    rect.right()
}

/// Text wrapped to `width` from `top`; returns its bottom.
fn wrapped(painter: &egui::Painter, left: f32, top: f32, width: f32, text: &str, color: Color32) -> f32 {
    let galley = painter.layout(text.to_string(), theme::ui_font(theme::TEXT_SMALL), color, width.max(10.0));
    let height = galley.size().y;
    widgets::paint_text(painter, Pos2::new(left, top), galley, color);
    top + height
}

/// What the one switch for several picked plug-ins does: on where every one is on, and a click
/// switches them all off; otherwise it shows off, and a click switches on every one that is off.
/// `None` where no plug-in is picked (files only).
fn switch_for_many(keys: &[String], plugins: &PluginsView) -> Option<(bool, Vec<String>, bool)> {
    let (on, off) = split_by_state(keys, plugins);
    if on.is_empty() && off.is_empty() {
        return None;
    }
    Some(if off.is_empty() { (true, on, false) } else { (false, off, true) })
}

/// The panel with several rows picked: how many, one switch for every picked plug-in, the files'
/// **Scan again**, and the list of what is picked with each one's state.
fn many_selected(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    plugins: &PluginsView,
    lines: &[Line],
    out: &mut ManagerOut,
) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let picked = &plugins.manager.picked;
    let line_at = |y: f32, height: f32| Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), height));
    let rule = |y: f32| painter.hline(rect.x_range(), y, theme::hairline(theme::border()));
    let mut y = rect.top();
    heading(painter, line_at(y, 18.0), "Selection");
    y += 18.0;
    let name_font = theme::ui_font(theme::TEXT + 6.0);
    let count = format!("{} selected", picked.len());
    widgets::text_in(painter, line_at(y, 30.0), Align2::LEFT_CENTER, &count, name_font, theme::text_primary());
    y += 30.0;
    let plugin_count = picked.iter().filter(|k| !k.starts_with("file:")).count();
    let files: Vec<String> = picked.iter().filter_map(|k| k.strip_prefix("file:")).map(str::to_string).collect();
    let mut parts = Vec::new();
    if plugin_count > 0 {
        parts.push(format!("{plugin_count} plug-ins"));
    }
    if !files.is_empty() {
        parts.push(format!("{} files", files.len()));
    }
    widgets::text_in(
        painter,
        line_at(y, 20.0),
        Align2::LEFT_CENTER,
        &parts.join(" · "),
        small.clone(),
        theme::text_secondary(),
    );
    y += 28.0;
    rule(y);
    y += 12.0;
    if let Some((on, specs, enable)) = switch_for_many(picked, plugins) {
        let toggle = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(36.0, 20.0));
        let label = format!("Enabled: {plugin_count} plug-ins");
        let id = Id::unique("manager-enabled-many");
        let response = switch(ui, painter, toggle, id, on, &label, !plugins.scanning);
        if response.clicked() {
            out.request = Some(PluginRequest::SetEnabledMany(specs, enable));
        }
        let (on_count, off_count) = {
            let (a, b) = split_by_state(picked, plugins);
            (a.len(), b.len())
        };
        let words = if off_count == 0 {
            "Enabled".to_string()
        } else if on_count == 0 {
            "Disabled".to_string()
        } else {
            format!("{on_count} enabled, {off_count} disabled")
        };
        let text = Rect::from_min_max(
            Pos2::new(toggle.right() + 10.0, toggle.top()),
            Pos2::new(rect.right(), toggle.bottom()),
        );
        widgets::text_in(painter, text, Align2::LEFT_CENTER, &words, small.clone(), theme::text_primary());
        // One mark for them all: on while every picked plug-in is marked; a click marks them all,
        // or, when all are, takes every mark away.
        let (owned, others) = split_by_owned(picked, plugins);
        let all = others.is_empty();
        let toggle = Rect::from_min_size(Pos2::new(rect.left(), toggle.bottom() + 10.0), Vec2::new(36.0, 20.0));
        let label = format!("Owned: {plugin_count} plug-ins");
        let response = switch(ui, painter, toggle, Id::unique("manager-owned-many"), all, &label, !plugins.scanning);
        if response.clicked() {
            out.request = Some(if all {
                PluginRequest::SetOwnedMany(owned.clone(), false)
            } else {
                PluginRequest::SetOwnedMany(others.clone(), true)
            });
        }
        let words = if all {
            "Owned".to_string()
        } else if owned.is_empty() {
            "Not marked as owned".to_string()
        } else {
            format!("{} owned, {} not", owned.len(), others.len())
        };
        let text = Rect::from_min_max(
            Pos2::new(toggle.right() + 10.0, toggle.top()),
            Pos2::new(rect.right(), toggle.bottom()),
        );
        widgets::text_in(painter, text, Align2::LEFT_CENTER, &words, small.clone(), theme::text_primary());
        y = toggle.bottom() + 12.0;
        rule(y);
        y += 12.0;
    }
    if !files.is_empty() {
        let text = format!("Scan {} files again", files.len());
        let id = Id::unique("manager-rescan-many");
        if widgets::button_with_id(ui, painter, line_at(y, 28.0), id, &text, &text, false).clicked()
            && !plugins.scanning
        {
            out.request = Some(PluginRequest::ScanFiles(files.clone()));
        }
        y += 40.0;
        rule(y);
        y += 10.0;
    }
    // The picked rows, in the order they were picked, each with its state.
    let list = Rect::from_min_max(Pos2::new(rect.left(), y), rect.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("plugin-manager-picked").auto_shrink([false, false]).show_rows(
            ui,
            ROW,
            picked.len(),
            |ui, range| {
                for key in picked.get(range).unwrap_or_default() {
                    let (row, _) = ui.allocate_exact_size(Vec2::new(list.width() - 12.0, ROW), Sense::hover());
                    let Some(line) = lines.iter().find(|l| &l.key == key) else { continue };
                    let row_painter = ui.painter_at(row);
                    state_mark(&row_painter, Pos2::new(row.left() + 5.0, row.center().y), line.state);
                    let words = Rect::from_min_max(Pos2::new(row.left() + 16.0, row.top()), row.max);
                    let ink = if line.state == State::Disabled { theme::text_hint() } else { theme::text_secondary() };
                    widgets::text_in(&row_painter, words, Align2::LEFT_CENTER, &line.cells[1], small.clone(), ink);
                }
            },
        );
    });
}

/// The panel on the right: the row picked last.
fn selected(ui: &mut Ui, painter: &egui::Painter, rect: Rect, plugins: &PluginsView, out: &mut ManagerOut) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let lines = all_lines(plugins);
    if plugins.manager.picked.len() > 1 {
        many_selected(ui, painter, rect, plugins, &lines, out);
        return;
    }
    let Some(line) = plugins.manager.anchor.as_ref().and_then(|key| lines.iter().find(|l| &l.key == key)) else {
        let at = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 20.0));
        widgets::text_in(painter, at, Align2::LEFT_CENTER, "Pick a row to see it here", small, theme::text_hint());
        return;
    };
    let plugin = match line.kind {
        Kind::Plugin(i) => plugins.found.get(i),
        _ => None,
    };
    let rule = |y: f32| painter.hline(rect.x_range(), y, theme::hairline(theme::border()));
    let line_at = |y: f32, height: f32| Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(rect.width(), height));
    let mut y = rect.top();
    heading(painter, line_at(y, 18.0), if plugin.is_some() { "Selected plug-in" } else { "Selected file" });
    y += 18.0;
    let name_font = theme::ui_font(theme::TEXT + 6.0);
    widgets::text_in(painter, line_at(y, 30.0), Align2::LEFT_CENTER, &line.cells[1], name_font, theme::text_primary());
    y += 30.0;
    let under = plugin.map_or_else(|| folder_of(&line.file), |p| p.vendor.clone());
    widgets::text_in(painter, line_at(y, 20.0), Align2::LEFT_CENTER, &under, small.clone(), theme::text_secondary());
    y += 26.0;
    let mut x = rect.left();
    for text in [&line.cells[4], &line.cells[3]] {
        if !text.is_empty() {
            x = tag(ui, painter, Pos2::new(x, y), text) + 6.0;
        }
    }
    y += 32.0;
    rule(y);
    y += 12.0;
    if let Some(plugin) = plugin {
        let toggle = Rect::from_min_size(Pos2::new(rect.left(), y), Vec2::new(36.0, 20.0));
        let label = format!("Enabled: {}", plugin.name);
        let id = Id::unique(("manager-enabled", &plugin.spec));
        let response = switch(ui, painter, toggle, id, plugin.enabled, &label, !plugins.scanning);
        if response.clicked() {
            out.request = Some(PluginRequest::SetEnabled(plugin.spec.clone(), !plugin.enabled));
        }
        let words = Rect::from_min_max(
            Pos2::new(toggle.right() + 10.0, toggle.top()),
            Pos2::new(rect.right(), toggle.bottom()),
        );
        widgets::text_in(painter, words, Align2::LEFT_CENTER, "Enabled", small.clone(), theme::text_primary());
        let toggle = Rect::from_min_size(Pos2::new(rect.left(), toggle.bottom() + 10.0), Vec2::new(36.0, 20.0));
        let label = format!("Owned: {}", plugin.name);
        let id = Id::unique(("manager-owned", &plugin.spec));
        let response = switch(ui, painter, toggle, id, plugin.owned, &label, !plugins.scanning);
        if response.clicked() {
            out.request = Some(PluginRequest::SetOwnedMany(vec![plugin.spec.clone()], !plugin.owned));
        }
        let words = Rect::from_min_max(
            Pos2::new(toggle.right() + 10.0, toggle.top()),
            Pos2::new(rect.right(), toggle.bottom()),
        );
        widgets::text_in(painter, words, Align2::LEFT_CENTER, "Owned", small.clone(), theme::text_primary());
        y = toggle.bottom() + 12.0;
        rule(y);
        y += 10.0;
    }
    // Its version and state, each beside its name.
    let value_left = rect.left() + 96.0;
    let pair = |y: &mut f32, name: &str, value: &str, state: Option<State>| {
        let at = line_at(*y, 22.0);
        widgets::text_in(painter, at, Align2::LEFT_CENTER, name, small.clone(), theme::text_secondary());
        let mut value_rect = Rect::from_min_max(Pos2::new(value_left, at.top()), at.max);
        let mut ink = theme::text_primary();
        if let Some(state) = state {
            state_mark(painter, Pos2::new(value_rect.left() + 5.0, at.center().y), state);
            value_rect.min.x += 16.0;
            ink = state.inks().1;
        }
        widgets::text_in(painter, value_rect, Align2::LEFT_CENTER, value, small.clone(), ink);
        *y += 22.0;
    };
    if let Some(plugin) = plugin
        && !plugin.version.is_empty()
    {
        pair(&mut y, "Version", &plugin.version, None);
    }
    pair(&mut y, "Status", line.state.word(), Some(line.state));
    if plugin.is_none() {
        y = wrapped(painter, rect.left(), y + 4.0, rect.width(), &line.cells[FILE], line.state.inks().1) + 6.0;
    }
    y += 8.0;
    rule(y);
    y += 12.0;
    widgets::text_in(painter, line_at(y, 18.0), Align2::LEFT_CENTER, "File location", small, theme::text_primary());
    y = wrapped(painter, rect.left(), y + 22.0, rect.width(), &line.file, theme::text_secondary()) + 8.0;
    let label = format!("Show {} in its folder", file_name(&line.file));
    let id = Id::unique(("manager-reveal", &line.file));
    if link(ui, painter, line_at(y, 20.0), id, "Show in folder", &label, Align2::LEFT_CENTER) {
        out.request = Some(PluginRequest::ShowFile(line.file.clone()));
    }
    y += 32.0;
    let (text, label) = match plugin {
        Some(p) => ("Rescan plug-in", format!("Rescan {}", p.name)),
        None => ("Scan again", format!("Scan {} again", file_name(&line.file))),
    };
    let id = Id::unique(("manager-rescan", &line.file));
    if widgets::button_with_id(ui, painter, line_at(y, 28.0), id, text, &label, false).clicked() && !plugins.scanning {
        out.request = Some(PluginRequest::ScanFiles(vec![line.file.clone()]));
    }
    y += 40.0;
    rule(y);
    if plugin.is_some() {
        let hint = "A plug-in switched off is hidden from the browser and never loaded.";
        wrapped(painter, rect.left(), y + 10.0, rect.width(), hint, theme::text_hint());
    }
}

/// The status bar: how many plug-ins, shown and picked on the left, the scan on the right.
fn status_bar(painter: &egui::Painter, rect: Rect, plugins: &PluginsView, lines: &[Line]) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let total = plugins.found.len() + plugins.failed.len() + plugins.incompatible.len();
    let mut left = format!("{} plug-ins", plugins.found.len());
    if lines.len() != total {
        left += &format!(" · {} shown", lines.len());
    }
    left += &format!(" · {} selected", plugins.manager.picked.len());
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, &left, small.clone(), theme::text_secondary());
    let (word, dot) =
        if plugins.scanning { ("Scanning…", theme::accent()) } else { ("Scan complete", theme::meter_normal()) };
    let width = painter.layout_no_wrap(word.to_string(), small.clone(), theme::text_secondary()).size().x;
    widgets::text_in(painter, rect, Align2::RIGHT_CENTER, word, small, theme::text_secondary());
    painter.circle_filled(Pos2::new(rect.right() - width - 10.0, rect.center().y), 3.5, dot);
}

#[cfg(test)]
#[path = "plugin_manager_tests.rs"]
mod tests;
