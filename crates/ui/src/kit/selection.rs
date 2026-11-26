// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The fill of selected track headers and clips, in the Arrangement and the Launcher
//! (`docs/spec/arrangement-editing.md` › Selection color): the theme's `selection.background`,
//! one color Settings › Appearance › Selection color chose over it, or each one's own color as
//! it was before there was one selection color.
//!
//! The choice is the window's, as the theme's colors are: the app gives it once a frame
//! ([`set`]), with the theme's colors, and a chosen color is written over the theme's
//! `--selection-background` on `window` ([`super::native_css::set_window_colors`]), so the lanes'
//! and columns' tint, which the base sheet works out from that variable, follows it too.

use std::cell::Cell;
use std::sync::Arc;

use egui::Color32;
use suhina_engine::model::TrackId;

const PREVIEW: &str = "track-color-preview";

/// The color tool's targets for this frame. This presentation state changes neither the
/// selection nor its saved appearance setting. Only headers consult it; clips keep their look.
pub(crate) fn preview_tracks(ctx: &egui::Context, tracks: &[TrackId]) {
    ctx.data_mut(|data| {
        let key = egui::Id::unique(PREVIEW);
        let old = data.get_temp::<Arc<[TrackId]>>(key);
        if old.as_deref().unwrap_or_default() != tracks {
            data.insert_temp(key, Arc::<[TrackId]>::from(tracks));
        }
    });
}

/// Whether a header should show its own color while the color tool edits it.
pub(crate) fn previews_track(ctx: &egui::Context, track: TrackId) -> bool {
    ctx.data(|data| data.get_temp::<Arc<[TrackId]>>(egui::Id::unique(PREVIEW)))
        .is_some_and(|tracks| tracks.contains(&track))
}

/// The custom property of the `selection.background` token.
pub const VARIABLE: &str = "--selection-background";

/// Which color selected track headers and clips take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionColor {
    /// The theme's `selection.background`, the same for every one.
    #[default]
    Theme,
    /// Each keeps its own color, lifted a little, as before there was one selection color.
    Own,
    /// One color chosen in Settings, the same for every one.
    Custom,
}

impl SelectionColor {
    /// The name the settings file keeps it by.
    pub fn name(self) -> &'static str {
        match self {
            Self::Theme => "theme",
            Self::Own => "own",
            Self::Custom => "custom",
        }
    }

    /// The choice the settings file's `name` stands for.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "theme" => Some(Self::Theme),
            "own" => Some(Self::Own),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

/// The selection color setting: the choice, and the color **Custom** uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionLook {
    pub mode: SelectionColor,
    pub custom: Color32,
}

impl Default for SelectionLook {
    fn default() -> Self {
        Self { mode: SelectionColor::Theme, custom: Color32::from_rgb(0xD9, 0x3A, 0x3A) }
    }
}

thread_local! {
    /// The look the app gave last ([`set`]).
    static LOOK: Cell<SelectionLook> = Cell::new(SelectionLook::default());
}

/// Draws selected headers and clips with `look` from the next frame on. **Custom** writes its
/// color as `--selection-background` on `window`, over the theme's; the other choices leave the
/// theme's.
pub fn set(look: SelectionLook) {
    LOOK.with(|current| current.set(look));
    if look.mode == SelectionColor::Custom {
        super::native_css::set_window_colors(&[(VARIABLE, look.custom)]);
    } else {
        super::native_css::set_window_colors(&[]);
    }
}

/// The look in use ([`set`]).
pub fn look() -> SelectionLook {
    LOOK.with(Cell::get)
}

/// The fill of a selected track header or clip whose element is `el`: the color chosen in
/// Settings, else the `selection.background` the element sees (the theme's), else Suhina
/// Studio's. `None` when each keeps its own color (**Clip's own color**).
pub fn fill(el: &super::element::Entered) -> Option<Color32> {
    let look = look();
    match look.mode {
        SelectionColor::Own => None,
        SelectionColor::Custom => Some(look.custom),
        SelectionColor::Theme => Some(
            el.style()
                .and_then(|style| style.color_variable(VARIABLE))
                .or_else(|| super::theme::default_color(VARIABLE.trim_start_matches("--")))
                .unwrap_or(Color32::from_rgb(0xDB, 0xDA, 0xFF)),
        ),
    }
}

/// `ink` where it reads on `fill`, else the text color that does: what a waveform, notes or a
/// name are drawn in on a selected fill, which may be light or dark whatever the theme is.
pub fn readable(ink: Color32, fill: Color32) -> Color32 {
    if super::theme::text_contrast(ink, fill) >= 3.0 { ink } else { super::theme::text_on(fill) }
}
