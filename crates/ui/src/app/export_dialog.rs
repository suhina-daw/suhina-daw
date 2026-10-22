// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! File › Export…: renders the mix to the audio file the user chose — the basic dialog of
//! [`docs/spec/export.md`](../../docs/spec/export.md) › In the app: the file (typed or
//! browsed), the format, the sample rate, the depth, normalise, dither and overwrite. The
//! export itself is the shared `project.export` command, so the dialog only gathers its
//! params; the source and range of the render matrix are the slices after this one.

use std::path::Path;

use egui::{Align2, Id, Key, Pos2, Rect, Ui, Vec2};
use serde_json::{Value, json};

use crate::app::project_dialog::{BUTTON, CloseChoice, LINE, SAVE_WIDTH, panel};
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, button};

/// The formats the dialog offers, in its order; `From the file name` leaves `format` out, so
/// the file name's extension decides, as the command does.
const FORMATS: [(&str, Option<&str>); 4] =
    [("From the file name", None), ("WAV", Some("wav")), ("FLAC", Some("flac")), ("Ogg Vorbis", Some("ogg"))];

/// The sample rates the dialog offers, in hertz.
const SAMPLE_RATES: [u64; 4] = [44_100, 48_000, 88_200, 96_000];

/// The depths the dialog offers, with the words the command knows.
const DEPTHS: [(&str, &str); 3] = [("32-bit float", "32f"), ("24-bit PCM", "24"), ("16-bit PCM", "16")];

/// What the export dialog keeps between frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportDialog {
    /// The file to write, as text; Browse… fills it from the system's dialog.
    pub path: String,
    /// An index into [`FORMATS`].
    pub format: usize,
    /// An index into [`SAMPLE_RATES`].
    pub rate: usize,
    /// An index into [`DEPTHS`].
    pub depth: usize,
    pub normalize: bool,
    pub dither: bool,
    pub overwrite: bool,
    /// The export is running on its worker; the note says so and the buttons wait.
    pub busy: bool,
    /// The export's state reads are being gathered over the frames: the Cancel works, and
    /// closing the dialog cancels. The render that follows has no cancel yet (the render
    /// matrix's slice); while the render runs the buttons wait as before.
    pub gathering: bool,
    /// The file field takes the keys when the dialog appears.
    pub focus: bool,
}

impl ExportDialog {
    /// A dialog for `path`, whose extension decides the format until the user picks one.
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path: path.display().to_string(),
            format: 0,
            rate: SAMPLE_RATES.iter().position(|rate| *rate == 48_000).unwrap_or(0),
            depth: 0,
            normalize: false,
            // The command's and the spec's default: dither is on for a reduced depth, and the
            // checkbox only shows where it applies.
            dither: true,
            overwrite: false,
            busy: false,
            gathering: false,
            focus: true,
        }
    }

    /// The format the export will run as: the one chosen, else the file name's extension's;
    /// none when neither names one the command knows.
    fn resolved_format(&self) -> Option<&'static str> {
        if let Some((_, Some(name))) = FORMATS.get(self.format) {
            return Some(name);
        }
        let ext = Path::new(self.path.trim()).extension()?.to_str()?.to_ascii_lowercase();
        ["wav", "flac", "ogg"].into_iter().find(|known| *known == ext.as_str())
    }

    /// The depth the export will run at, when the format takes one: ogg does not.
    fn resolved_depth(&self) -> Option<&'static str> {
        if self.resolved_format() == Some("ogg") { None } else { Some(DEPTHS[self.depth].1) }
    }

    /// Whether the depth reduces the mix to PCM, so dither applies and is asked for.
    fn dither_applies(&self) -> bool {
        self.resolved_depth().is_some_and(|depth| depth != "32f")
    }

    /// A file picked from the system's dialog: the file field takes it, and the format and the
    /// depth follow the picked file's extension, the way the Format row's own choice would —
    /// the picked file names the format, and a flac cannot take the float depth (the command's
    /// own default for it is 24). An extension neither the format row nor the command knows
    /// leaves both as they stand, for the command to refuse in its own words.
    pub fn accept_pick(&mut self, path: &Path) {
        self.path = path.display().to_string();
        let Some(ext) = Path::new(&self.path).extension().and_then(|ext| ext.to_str()) else { return };
        let ext = ext.to_ascii_lowercase();
        let Some((index, _)) = FORMATS.iter().enumerate().find(|(_, (_, known))| *known == Some(ext.as_str())) else {
            return;
        };
        self.format = index;
        if ext == "flac" && self.depth == 0 {
            self.depth = 1;
        }
    }

    /// Why the export would be refused as the settings stand, shown in the dialog's note
    /// before the click; the command refuses with its own texts.
    fn refusal(&self) -> Option<&'static str> {
        if self.resolved_format() == Some("flac") && self.depth == 0 {
            return Some("flac is written at 24 or 16 bits, not 32f");
        }
        None
    }

    /// The job the dialog's settings make, when the file is named at all.
    fn job(&self) -> Option<ExportJob> {
        let path = Path::new(self.path.trim());
        (!path.as_os_str().is_empty()).then(|| ExportJob {
            path: path.to_path_buf(),
            format: FORMATS[self.format].1,
            sample_rate: SAMPLE_RATES[self.rate],
            bit_depth: self.resolved_depth(),
            normalize: self.normalize,
            // Asked for only when the depth takes it; left out, the command's default is the
            // same: on for a reduced depth, off at 32f.
            dither: self.dither_applies().then_some(self.dither),
            overwrite: self.overwrite,
        })
    }
}

/// The export the dialog gathered: the pieces `project.export` takes, with the ones its own
/// defaults already carry left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportJob {
    pub path: std::path::PathBuf,
    pub format: Option<&'static str>,
    pub sample_rate: u64,
    pub bit_depth: Option<&'static str>,
    pub normalize: bool,
    /// `Some(checked)` when the depth takes dither, `None` where the command takes no `dither`.
    pub dither: Option<bool>,
    pub overwrite: bool,
}

impl ExportJob {
    /// The command's params.
    pub fn params(&self) -> Value {
        let mut params = json!({
            "path": self.path.display().to_string(),
            "sample_rate": self.sample_rate,
        });
        if let Some(format) = self.format {
            params["format"] = json!(format);
        }
        if let Some(depth) = self.bit_depth {
            params["bit_depth"] = json!(depth);
        }
        if let Some(dither) = self.dither {
            params["dither"] = json!(dither);
        }
        if self.normalize {
            params["normalize"] = json!(true);
        }
        if self.overwrite {
            params["overwrite"] = json!(true);
        }
        params
    }
}

/// What the user did with the export dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportChoice {
    /// Export with the settings the dialog shows.
    Export(ExportJob),
    /// Browse… beside the file: the system's dialog for it; the dialog stays.
    Browse,
    Cancel,
}

impl CloseChoice for ExportChoice {
    fn close() -> Self {
        ExportChoice::Cancel
    }
}

/// Draws the export dialog over `area`. Returns what was chosen, or `None` while it stays open.
pub fn show_export(ui: &Ui, area: Rect, state: &mut ExportDialog) -> Option<ExportChoice> {
    let _style_class = crate::kit::style_class::enter("dialog");
    egui::Area::new(Id::new("export-dialog"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw_export(ui, area, state)
        })
        .inner
}

fn draw_export(ui: &mut Ui, area: Rect, state: &mut ExportDialog) -> Option<ExportChoice> {
    let mut closed: Option<ExportChoice> = None;
    // Title row, then file, format, rate, depth, the checkboxes, the note, and the buttons.
    let panel =
        panel(ui, area, Vec2::new(SAVE_WIDTH, ROW + 7.0 * LINE + BUTTON.y + 3.0 * PAD), "Export audio", &mut closed);
    let painter = ui.painter_at(area);
    let small = theme::ui_font(theme::TEXT_SMALL);
    let label_width = 76.0;
    let mut y = panel.top() + ROW + PAD;
    let mut entered = false;

    // The file, with Browse… beside it.
    let file_row = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
    widgets::text_in(&painter, file_row, Align2::LEFT_CENTER, "File", small.clone(), theme::text_secondary());
    let browse =
        Rect::from_min_size(Pos2::new(file_row.right() - 64.0, file_row.top() + 1.0), Vec2::new(64.0, BUTTON.y));
    let browse_clicked = button(ui, &painter, browse, "Browse…", "Browse for the file to export to");
    let input = Rect::from_min_max(
        Pos2::new(file_row.left() + label_width, file_row.top()),
        Pos2::new(browse.left() - 4.0, file_row.bottom()),
    );
    let focus = std::mem::take(&mut state.focus);
    let edit = ui.put(input, egui::TextEdit::singleline(&mut state.path).font(small.clone()));
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "File to export to"));
    if focus {
        edit.request_focus();
    }
    entered |= edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    y += LINE + 4.0;

    // A label on the left; the widget goes over the rest of the row, in a scoped `Ui`.
    let label_row = |y: &mut f32, label: &str| {
        let whole = Rect::from_min_size(Pos2::new(panel.left() + PAD, *y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
        widgets::text_in(&painter, whole, Align2::LEFT_CENTER, label, small.clone(), theme::text_secondary());
        let widget = Rect::from_min_max(
            Pos2::new(whole.left() + label_width, whole.top() + 1.0),
            Pos2::new(whole.left() + label_width + 170.0, whole.bottom() - 1.0),
        );
        *y += LINE + 4.0;
        widget
    };

    // The format, whose choice decides the depth's and dither's rows.
    let format_at = label_row(&mut y, "Format");
    let shown = FORMATS[state.format].0;
    ui.scope_builder(egui::UiBuilder::new().max_rect(format_at), |ui| {
        let response = egui::ComboBox::from_id_salt("export-format")
            .selected_text(egui::RichText::new(shown).font(small.clone()))
            .width(170.0)
            .show_ui(ui, |ui| {
                for (index, (name, ext)) in FORMATS.iter().enumerate() {
                    if ui.selectable_label(state.format == index, *name).clicked() {
                        state.format = index;
                        // The file name follows the choice, as the spec's dialog does; from
                        // the file name it does not touch it. And flac cannot take the float
                        // depth: the command's own default for it is 24.
                        if let Some(ext) = ext.as_deref() {
                            state.path = with_extension(&state.path, ext);
                            if ext == "flac" && state.depth == 0 {
                                state.depth = 1;
                            }
                        }
                    }
                }
            })
            .response;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Format {shown}")));
    });

    // The sample rate.
    let rate_at = label_row(&mut y, "Sample rate");
    let rate_shown = format!("{} Hz", SAMPLE_RATES[state.rate]);
    let mut chosen_rate = state.rate;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rate_at), |ui| {
        let response = egui::ComboBox::from_id_salt("export-rate")
            .selected_text(egui::RichText::new(&rate_shown).font(small.clone()))
            .width(170.0)
            .show_ui(ui, |ui| {
                for (index, hz) in SAMPLE_RATES.iter().enumerate() {
                    if ui.selectable_label(state.rate == index, format!("{hz} Hz")).clicked() {
                        chosen_rate = index;
                    }
                }
            })
            .response;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Sample rate {rate_shown}"))
        });
    });
    state.rate = chosen_rate;

    // The depth; ogg takes none, so its row says why and holds nothing.
    let depth_at = label_row(&mut y, "Bit depth");
    let depth = DEPTHS[state.depth];
    if state.resolved_format() == Some("ogg") {
        widgets::text_in(
            &painter,
            depth_at,
            Align2::LEFT_CENTER,
            "ogg takes no bit depth",
            small.clone(),
            theme::text_hint(),
        );
    } else {
        let depth_shown = depth.0;
        let mut chosen_depth = state.depth;
        ui.scope_builder(egui::UiBuilder::new().max_rect(depth_at), |ui| {
            let response = egui::ComboBox::from_id_salt("export-depth")
                .selected_text(egui::RichText::new(depth_shown).font(small.clone()))
                .width(170.0)
                .show_ui(ui, |ui| {
                    for (index, (name, _)) in DEPTHS.iter().enumerate() {
                        if ui.selectable_label(state.depth == index, *name).clicked() {
                            chosen_depth = index;
                        }
                    }
                })
                .response;
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Bit depth {depth_shown}"))
            });
        });
        state.depth = chosen_depth;
    }

    // Normalise, dither and overwrite on one row; dither greys out where the depth takes none.
    let checks = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
    let at = |left: f32, width: f32| {
        Rect::from_min_size(Pos2::new(checks.left() + left, checks.top() + 1.0), Vec2::new(width, LINE - 2.0))
    };
    ui.put(at(0.0, 96.0), egui::Checkbox::new(&mut state.normalize, "Normalize"))
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, true, "Normalize".to_string()));
    if state.dither_applies() {
        ui.put(at(100.0, 84.0), egui::Checkbox::new(&mut state.dither, "Dither"))
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, true, "Dither".to_string()));
    } else {
        // Greyed where it does not apply: 32f needs none, and ogg takes no depth.
        let mut off = false;
        ui.scope(|scope| {
            scope.disable();
            scope.put(at(100.0, 84.0), egui::Checkbox::new(&mut off, "Dither")).widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, false, "Dither does not apply".to_string())
            });
        });
    }
    ui.put(at(188.0, 120.0), egui::Checkbox::new(&mut state.overwrite, "Overwrite"))
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Checkbox, true, "Overwrite".to_string()));
    y += LINE + 8.0;

    // What the export will run as, or why it will not; while it runs, that.
    let note = Rect::from_min_size(Pos2::new(panel.left() + PAD, y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
    let path = Path::new(state.path.trim());
    let empty = path.as_os_str().is_empty();
    let (text, colour) = if state.busy {
        ("Exporting…".to_string(), theme::text_hint())
    } else if let Some(said) = state.refusal().filter(|_| !empty) {
        (said.to_string(), theme::recording())
    } else if empty {
        ("Name the file to write".to_string(), theme::text_hint())
    } else if state.resolved_format().is_none() {
        ("The file name decides: .wav, .flac or .ogg".to_string(), theme::text_hint())
    } else if path.exists() && !state.overwrite {
        (format!("{} is there already; overwrite replaces it", path.display()), theme::recording())
    } else {
        let format = state.resolved_format().unwrap_or_default();
        let rate = SAMPLE_RATES[state.rate];
        let what =
            if state.resolved_depth().is_some() { format!(", {}", DEPTHS[state.depth].0) } else { String::new() };
        (format!("{} at {} Hz{}", format.to_uppercase(), rate, what), theme::text_hint())
    };
    widgets::text_in(&painter, note, Align2::LEFT_CENTER, &text, small, colour);

    let export_at =
        Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, panel.bottom() - PAD - BUTTON.y), BUTTON);
    let cancel_at = Rect::from_min_size(Pos2::new(export_at.left() - 4.0 - BUTTON.x, export_at.top()), BUTTON);
    // While the export renders on its worker the buttons wait: the job was sent whole, and
    // cancelling it is the render matrix's slice. While the states are being gathered the
    // Cancel works: the close cancels the gather, the late replies changing nothing.
    let chosen = if state.busy {
        None
    } else {
        state.job().filter(|_| button(ui, &painter, export_at, "Export", "Export the mix to the file") || entered)
    };
    if (!state.busy || state.gathering)
        && (button(ui, &painter, cancel_at, "Cancel", "Cancel exporting") || ui.input(|i| i.key_pressed(Key::Escape)))
    {
        return Some(ExportChoice::Cancel);
    }
    if closed.is_some() {
        return Some(ExportChoice::Cancel);
    }
    if browse_clicked {
        return Some(ExportChoice::Browse);
    }
    chosen.map(ExportChoice::Export)
}

/// `path` with its extension swapped for `ext`, keeping the folder and the rest of the name —
/// the text the user sees stays theirs, its separators included (a rebuild through `Path` would
/// swap them for this platform's own, showing a Windows path differently than it went in); a
/// path with no file name ("", "/", a folder's own, ".") stays as it is.
fn with_extension(path: &str, ext: &str) -> String {
    let path = path.trim();
    // The file's part starts after the last separator of either kind.
    let file_at = path.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let file = &path[file_at..];
    if file.is_empty() {
        return path.to_string();
    }
    match file.rfind('.') {
        Some(dot) if !file[..dot].chars().all(|c| c == '.') => format!("{}.{}", &path[..file_at + dot], ext),
        Some(_) => path.to_string(), // ".", "..": no stem to keep an extension off.
        None => format!("{path}.{ext}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_picked_file_reconciles_the_format_and_the_depth() {
        // A format the user chose explicitly follows the picked file's extension, so the job's
        // format and the file name agree — the command refuses the mismatch the other way.
        let mut wav = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
        wav.format = 1; // WAV, chosen explicitly; the depth is its default 32f.
        wav.accept_pick(Path::new("/mixes/Song.flac"));
        assert_eq!(wav.format, 2, "the picked flac turns the format row");
        assert_eq!(wav.resolved_depth(), Some("24"), "flac takes no float depth");
        let job = wav.job().unwrap();
        assert_eq!(job.format, Some("flac"));
        assert_eq!(job.bit_depth, Some("24"));
        assert_eq!(job.path, PathBuf::from("/mixes/Song.flac"));

        // With the automatic format, browsing to a flac moves the float depth out the same way.
        let mut auto = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
        auto.format = 0; // From the file name; the depth is 32f.
        auto.accept_pick(Path::new("/mixes/Song.flac"));
        assert_eq!(auto.format, 2);
        assert_eq!(auto.resolved_depth(), Some("24"), "{:?}", auto.job());

        // Ogg takes no depth, whatever the row held; a wav pick leaves a wav choice alone.
        let mut ogg = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
        ogg.format = 1;
        ogg.accept_pick(Path::new("/mixes/Song.ogg"));
        assert_eq!(ogg.format, 3);
        assert_eq!(ogg.resolved_depth(), None);
        let (_, name) = FORMATS[ogg.format];
        assert_eq!(name, Some("ogg"));

        // A file without one of the format's extensions reads as it did before the pick.
        let mut unknown = ExportDialog::new(PathBuf::from("/mixes/Song.wav"));
        unknown.format = 1;
        unknown.accept_pick(Path::new("/mixes/Song.aif"));
        assert_eq!(unknown.format, 1, "an unknown extension is the command's own refusal");
        assert_eq!(unknown.resolved_depth(), Some("32f"));
    }

    #[test]
    fn choosing_the_format_swaps_the_file_names_extension() {
        assert_eq!(with_extension("/mixes/Song.wav", "ogg"), "/mixes/Song.ogg");
        assert_eq!(with_extension("Song", "flac"), "Song.flac");
        assert_eq!(with_extension("  /a/b/night.drive.WAV ", "ogg"), "/a/b/night.drive.ogg");
        assert_eq!(with_extension("", "wav"), "");
        // The separators the user typed stay as they typed them, a Windows path included.
        assert_eq!(with_extension(r"C:\mixes\Song.wav", "ogg"), r"C:\mixes\Song.ogg");
        // A folder's own path has no file to rename, and neither has "." or "..".
        assert_eq!(with_extension("/mixes/", "ogg"), "/mixes/");
        assert_eq!(with_extension(".", "ogg"), ".");
        assert_eq!(with_extension("..", "ogg"), "..");
    }

    #[test]
    fn the_jobs_params_carry_what_the_command_takes() {
        let job = ExportJob {
            path: "/mixes/Song.wav".into(),
            format: None,
            sample_rate: 48_000,
            bit_depth: Some("32f"),
            normalize: false,
            dither: Some(false),
            overwrite: false,
        };
        // The depth's dither is asked for even when it is off, or the command would turn it on;
        // the bools that are false by default are left out.
        assert_eq!(
            job.params(),
            json!({ "path": "/mixes/Song.wav", "sample_rate": 48_000, "bit_depth": "32f", "dither": false })
        );
        let off = ExportJob {
            path: "/mixes/Song.wav".into(),
            format: Some("ogg"),
            sample_rate: 44_100,
            bit_depth: None,
            normalize: true,
            dither: None,
            overwrite: true,
        };
        assert_eq!(
            off.params(),
            json!({ "path": "/mixes/Song.wav", "format": "ogg", "sample_rate": 44_100, "normalize": true, "overwrite": true })
        );
    }

    #[test]
    fn ogg_takes_no_depth_and_32f_takes_no_dither() {
        let mut state = ExportDialog::new("/mixes/Song.wav".into());
        assert_eq!(state.resolved_format(), Some("wav"));
        assert_eq!(state.resolved_depth(), Some("32f"));
        assert!(!state.dither_applies(), "32f needs no dither");
        state.depth = 2;
        assert_eq!(state.resolved_depth(), Some("16"));
        assert!(state.dither_applies(), "16-bit takes dither");
        state.format = 3; // Ogg Vorbis
        assert_eq!(state.resolved_format(), Some("ogg"));
        assert_eq!(state.resolved_depth(), None);
        assert!(!state.dither_applies());
        assert_eq!(state.job().map(|job| job.bit_depth), Some(None), "the job leaves the depth out for ogg");
    }

    #[test]
    fn flac_warns_at_the_float_depth_and_24_is_its_default() {
        let mut state = ExportDialog::new("/mixes/Song.wav".into());
        assert_eq!(state.refusal(), None, "wav at 32f is fine");
        state.format = 2; // FLAC, still at 32-bit float
        assert_eq!(state.refusal(), Some("flac is written at 24 or 16 bits, not 32f"));
        state.depth = 1; // 24-bit PCM
        assert_eq!(state.refusal(), None);
        assert!(state.dither_applies(), "24-bit flac takes dither");
        let job = state.job().unwrap();
        assert_eq!(job.bit_depth, Some("24"));
        // Dither starts on, as the command's and the spec's default do, and is asked for
        // explicitly so the command does not guess.
        assert_eq!(job.dither, Some(true));
        state.depth = 2; // 16-bit PCM
        assert_eq!(state.refusal(), None);
        assert_eq!(state.job().unwrap().bit_depth, Some("16"));
    }
}
