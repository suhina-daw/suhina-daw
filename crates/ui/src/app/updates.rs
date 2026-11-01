// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Updates in the window (`docs/spec/updates.md`): the check once per start, the start panel's
//! *New version available* line, and Help › Check for updates…, which checks now, lists test
//! builds in developer mode and connects to the GitHub CLI only when asked to.

use crate::kit::surface::Backgrounds as _;
use std::path::PathBuf;

use egui::{Align2, Id, Key, Pos2, Rect, Ui, Vec2};
use suhina_update::{Build, BuildKind, Channel, Phase, Updater};

use crate::app::project_dialog::{self, BUTTON, LINE, VERSION};
use crate::app::start_panel::Notice;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, button};
use crate::settings::update_settings::UpdateSettings;

const WIDTH: f32 = 560.0;
const WIDE_BUTTON: Vec2 = Vec2::new(150.0, 18.0);
/// How many test builds the dialog lists.
const LISTED: usize = 8;

/// What the window does for the updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanted {
    /// Quit as File › Quit does, and start the updated build.
    Restart,
    /// Open a page in the browser.
    Open(String),
}

/// What was chosen in the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Choice {
    Close,
    Check,
    Install(Build),
    Restart,
    Connect,
    Disconnect,
    Open(String),
}

impl project_dialog::CloseChoice for Choice {
    fn close() -> Self {
        Self::Close
    }
}

/// The updater and what the window keeps about it.
#[derive(Default)]
pub struct Updates {
    updater: Updater,
    /// The check at start was made (or skipped, as the setting says).
    started: bool,
    /// Help › Check for updates… is open.
    pub open: bool,
    /// The GitHub CLI, looked for each time the dialog opens.
    gh: Option<PathBuf>,
    /// The token `gh auth token` gave after *Connect Suhina to your gh*; kept in memory only.
    token: Option<String>,
    /// Why connecting or installing could not start.
    note: Option<String>,
    /// The channel the last check asked.
    channel: Channel,
}

impl Updates {
    /// Once per start: deletes what an earlier update left, and checks when the setting says so.
    pub fn at_start(&mut self, ctx: &egui::Context, settings: &UpdateSettings) {
        if std::mem::replace(&mut self.started, true) {
            return;
        }
        suhina_update::clean_up();
        if settings.check {
            self.check(ctx, settings);
        }
    }

    fn check(&mut self, ctx: &egui::Context, settings: &UpdateSettings) {
        self.channel = settings.channel();
        let ctx = ctx.clone();
        self.updater.check(self.channel, VERSION, move || ctx.request_repaint());
    }

    fn install(&mut self, ctx: &egui::Context, build: Build) {
        if build.needs_sign_in && self.token.is_none() {
            self.note = Some("Connect Suhina to your gh first: downloading a test build needs a sign-in".into());
            return;
        }
        match suhina_update::program_folder() {
            Ok(dir) => {
                self.note = None;
                let ctx = ctx.clone();
                self.updater.install(build, dir, self.token.clone(), move || ctx.request_repaint());
            }
            Err(reason) => self.note = Some(reason),
        }
    }

    /// Opens Help › Check for updates…, looking for the GitHub CLI again.
    pub fn open_dialog(&mut self) {
        self.open = true;
        self.gh = suhina_update::net::gh();
    }

    /// The start panel's line: a newer build, its download, or what became of it.
    pub fn notice(&self) -> Option<Notice> {
        let name = |b: &Build| match b.pr {
            Some(pr) => format!("the test build of #{pr} ({})", b.version),
            None => format!("Suhina {}", b.version),
        };
        Some(match self.updater.phase() {
            Phase::Checked { newer: Some(build), .. } => Notice {
                text: format!("New version available: {}", name(&build)),
                button: Some("Download update now"),
                progress: None,
                error: false,
                page: Some(build.page),
            },
            Phase::Downloading { build, done } => {
                let part = if build.size > 0 { (done as f32 / build.size as f32).clamp(0.0, 1.0) } else { 0.0 };
                Notice {
                    text: format!("Downloading {}: {:.0} %", name(&build), part * 100.0),
                    button: None,
                    progress: Some(part),
                    error: false,
                    page: None,
                }
            }
            Phase::Installing(build) => Notice {
                text: format!("Installing {}…", name(&build)),
                button: None,
                progress: Some(1.0),
                error: false,
                page: None,
            },
            Phase::Installed(build) => Notice {
                text: format!("{} is installed: restart Suhina to use it", name(&build)),
                button: Some("Restart now"),
                progress: None,
                error: false,
                page: None,
            },
            Phase::InstallFailed { reason, .. } => Notice {
                text: format!("The update was not installed: {reason}"),
                button: Some("Try again"),
                progress: None,
                error: true,
                page: None,
            },
            _ => return None,
        })
    }

    /// The start panel line's button was pressed.
    pub fn notice_pressed(&mut self, ctx: &egui::Context) -> Option<Wanted> {
        match self.updater.phase() {
            Phase::Checked { newer: Some(build), .. } | Phase::InstallFailed { build, .. } => {
                self.install(ctx, build);
                None
            }
            Phase::Installed(_) => Some(Wanted::Restart),
            _ => None,
        }
    }

    /// Draws Help › Check for updates… over `area`.
    pub fn show(&mut self, ui: &Ui, area: Rect, settings: &UpdateSettings) -> Option<Wanted> {
        let channel = settings.channel();
        let choice = egui::Area::new(Id::new("updates-dialog"))
            .order(egui::Order::Foreground)
            .fixed_pos(area.min)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(area.size());
                self.draw(ui, area, channel)
            })
            .inner;
        match choice? {
            Choice::Close => self.open = false,
            Choice::Check => self.check(ui.ctx(), settings),
            Choice::Install(build) => self.install(ui.ctx(), build),
            Choice::Restart => return Some(Wanted::Restart),
            Choice::Connect => match self.gh.as_deref().map(suhina_update::net::gh_token) {
                Some(Ok(token)) => {
                    self.token = Some(token);
                    self.note = None;
                }
                Some(Err(reason)) => self.note = Some(reason),
                None => self.note = Some("gh was not found".into()),
            },
            Choice::Disconnect => self.token = None,
            Choice::Open(url) => return Some(Wanted::Open(url)),
        }
        None
    }

    fn draw(&self, ui: &mut Ui, area: Rect, channel: Channel) -> Option<Choice> {
        let mut chosen: Option<Choice> = None;
        let phase = self.updater.phase();
        let test_builds = match (&phase, channel) {
            (Phase::Checked { builds, .. }, Channel::TestBuilds) if self.channel == Channel::TestBuilds => {
                builds.iter().take(LISTED).cloned().collect()
            }
            _ => Vec::new(),
        };
        let gh_lines = if channel == Channel::TestBuilds { 1.0 } else { 0.0 };
        let lines = 4.0 + gh_lines + test_builds.len() as f32 + f32::from(u8::from(self.note.is_some()));
        let height = ROW + lines * LINE + BUTTON.y + 4.0 * PAD;
        let panel = project_dialog::panel(ui, area, Vec2::new(WIDTH, height), "Updates", &mut chosen);
        let painter = ui.painter_at(area);
        let font = theme::ui_font(theme::TEXT);
        let mut y = panel.top() + ROW + PAD;
        let line = |y: &mut f32| {
            let rect =
                Rect::from_min_size(Pos2::new(panel.left() + PAD, *y), Vec2::new(panel.width() - 2.0 * PAD, LINE));
            *y += LINE;
            rect
        };
        let fact = |rect: Rect, name: &str, value: &str| {
            widgets::text_in(&painter, rect, Align2::LEFT_CENTER, name, font.clone(), theme::text_hint());
            let value_at = Rect::from_min_max(Pos2::new(rect.left() + 90.0, rect.top()), rect.max);
            widgets::text_in(&painter, value_at, Align2::LEFT_CENTER, value, font.clone(), theme::text_primary());
        };
        fact(line(&mut y), "This build", VERSION);
        let channel_name = match channel {
            Channel::Nightly => "Nightly (Settings › Agents and updates › Updates)",
            Channel::TestBuilds => "Test builds (Settings › Agents and updates › Updates)",
        };
        fact(line(&mut y), "Channel", channel_name);

        // What the last check found, and the one action it leads to.
        let (status, action): (String, Option<(&str, Choice)>) = match &phase {
            Phase::Idle => ("Not checked yet".into(), None),
            Phase::Checking => ("Checking…".into(), None),
            Phase::CheckFailed(reason) => (format!("The check failed: {reason}"), None),
            Phase::Checked { newer: Some(build), .. } => (
                format!("New version available: {}", build.version),
                Some(("Download update now", Choice::Install(build.clone()))),
            ),
            Phase::Checked { newer: None, builds } => match (builds.first(), self.channel) {
                (Some(nightly), Channel::Nightly) if nightly.version == VERSION => {
                    ("This is the newest nightly".into(), None)
                }
                (Some(nightly), Channel::Nightly) => match BuildKind::of(VERSION) {
                    BuildKind::TestBuild { .. } | BuildKind::Local => (
                        format!("This is not a nightly; the newest nightly is {}", nightly.version),
                        Some(("Install the nightly", Choice::Install(nightly.clone()))),
                    ),
                    _ => (format!("Up to date (the newest nightly is {})", nightly.version), None),
                },
                (_, Channel::TestBuilds) => (format!("{} test builds for this platform", builds.len()), None),
                (None, Channel::Nightly) => ("No nightly found".into(), None),
            },
            Phase::Downloading { build, done } => {
                (format!("Downloading {}: {} of {} MB", build.version, done / 1_000_000, build.size / 1_000_000), None)
            }
            Phase::Installing(build) => (format!("Installing {}…", build.version), None),
            Phase::Installed(build) => {
                (format!("{} is installed: restart to use it", build.version), Some(("Restart now", Choice::Restart)))
            }
            Phase::InstallFailed { build, reason } => {
                (format!("Not installed: {reason}"), Some(("Try again", Choice::Install(build.clone()))))
            }
        };
        let error = matches!(phase, Phase::CheckFailed(_) | Phase::InstallFailed { .. });
        let status_line = line(&mut y);
        let colour = if error { theme::recording() } else { theme::text_primary() };
        widgets::text_in(&painter, status_line, Align2::LEFT_CENTER, &status, font.clone(), colour);
        if let Phase::Downloading { build, done } = &phase
            && build.size > 0
        {
            let bar = line(&mut y).shrink2(Vec2::new(0.0, 7.0));
            painter.fill_background(bar, 0.0, theme::control_background());
            let part = (*done as f32 / build.size as f32).clamp(0.0, 1.0);
            painter.rect_filled(
                Rect::from_min_size(bar.min, Vec2::new(bar.width() * part, bar.height())),
                0.0,
                theme::accent(),
            );
        } else {
            y += LINE;
        }

        // The GitHub CLI, for test builds: never connected without this click.
        if channel == Channel::TestBuilds {
            let rect = line(&mut y);
            let (text, toggle) = match (&self.token, &self.gh) {
                (Some(_), _) => ("Connected to your gh until Suhina quits", Some(("Disconnect", Choice::Disconnect))),
                (None, Some(_)) => (
                    "Downloading a test build needs a GitHub sign-in",
                    Some(("Connect Suhina to your gh", Choice::Connect)),
                ),
                (None, None) => {
                    ("Downloading a test build needs the GitHub CLI (gh), signed in with gh auth login", None)
                }
            };
            widgets::text_in(&painter, rect, Align2::LEFT_CENTER, text, font.clone(), theme::text_hint());
            if let Some((text, choice)) = toggle {
                // Wide enough for its words, never narrower than the row's two buttons (#716).
                let width = (widgets::text_width(ui, text, font.clone()) + 24.0).max(WIDE_BUTTON.x);
                let at = Rect::from_min_size(
                    Pos2::new(rect.right() - width, rect.center().y - WIDE_BUTTON.y / 2.0),
                    Vec2::new(width, WIDE_BUTTON.y),
                );
                if button(ui, &painter, at, text, text) {
                    chosen = Some(choice);
                }
            }
        }
        let busy = self.updater.busy();
        for build in &test_builds {
            let rect = line(&mut y);
            let mine = BuildKind::of(VERSION) == BuildKind::of(&build.version);
            let when = build.created.get(..16).unwrap_or(&build.created).replace('T', " ");
            let text = format!(
                "#{}  {}  {when}{}",
                build.pr.unwrap_or_default(),
                build.version,
                if mine { "  (running)" } else { "" }
            );
            widgets::text_in(&painter, rect, Align2::LEFT_CENTER, &text, font.clone(), theme::text_primary());
            // Pull request and Install side by side at the right, the same width; without a
            // connection only Pull request, in Install's place (#716).
            let half = Vec2::new((WIDE_BUTTON.x - PAD) / 2.0, BUTTON.y);
            let install = Rect::from_min_size(Pos2::new(rect.right() - half.x, rect.center().y - half.y / 2.0), half);
            let installs = !busy && self.token.is_some();
            let page = if installs { install.translate(Vec2::new(-(half.x + PAD), 0.0)) } else { install };
            let pr = build.pr.unwrap_or_default();
            if button(ui, &painter, page, "Pull request", &format!("Open pull request #{pr}")) {
                chosen = Some(Choice::Open(build.page.clone()));
            }
            if installs && button(ui, &painter, install, "Install", &format!("Install the test build of #{pr}")) {
                chosen = Some(Choice::Install(build.clone()));
            }
        }
        if let Some(note) = &self.note {
            widgets::text_in(&painter, line(&mut y), Align2::LEFT_CENTER, note, font.clone(), theme::recording());
        }

        // The buttons along the bottom.
        let bottom = panel.bottom() - PAD - BUTTON.y;
        let check = Rect::from_min_size(Pos2::new(panel.left() + PAD, bottom), BUTTON);
        if !busy && button(ui, &painter, check, "Check now", "Check for updates now") {
            chosen = Some(Choice::Check);
        }
        if let Some((text, choice)) = action
            && !busy
        {
            let at = Rect::from_min_size(Pos2::new(panel.right() - PAD - WIDE_BUTTON.x, bottom), WIDE_BUTTON);
            if button(ui, &painter, at, text, text) {
                chosen = Some(choice);
            }
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            chosen = Some(Choice::Close);
        }
        chosen
    }
}
