// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The fill of selected track headers and clips, in the Arrangement and the Launcher
//! (`docs/spec/arrangement-editing.md` › Selection colour): the theme's `selection.background`,
//! one colour Settings › Appearance › Selection colour chose over it, or each one's own colour as
//! it was before there was one selection colour.
//!
//! The choice is the window's, as the theme's colours are: the app gives it once a frame
//! ([`set`]), with the theme's colours, and a chosen colour is written over the theme's
//! `--selection-background` on `window` ([`super::native_css::set_window_colors`]), so the lanes'
//! and columns' tint, which the base sheet works out from that variable, follows it too.

use std::cell::Cell;

use egui::Color32;

/// The custom property of the `selection.background` token.
pub const VARIABLE: &str = "--selection-background";

/// Which colour selected track headers and clips take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionColour {
    /// The theme's `selection.background`, the same for every one.
    #[default]
    Theme,
    /// Each keeps its own colour, lifted a little, as before there was one selection colour.
    Own,
    /// One colour chosen in Settings, the same for every one.
    Custom,
}

impl SelectionColour {
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

/// The selection colour setting: the choice, and the colour **Custom** uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionLook {
    pub mode: SelectionColour,
    pub custom: Color32,
}

impl Default for SelectionLook {
    fn default() -> Self {
        Self { mode: SelectionColour::Theme, custom: Color32::from_rgb(0xD9, 0x3A, 0x3A) }
    }
}

thread_local! {
    /// The look the app gave last ([`set`]).
    static LOOK: Cell<SelectionLook> = Cell::new(SelectionLook::default());
}

/// Draws selected headers and clips with `look` from the next frame on. **Custom** writes its
/// colour as `--selection-background` on `window`, over the theme's; the other choices leave the
/// theme's.
pub fn set(look: SelectionLook) {
    LOOK.with(|current| current.set(look));
    if look.mode == SelectionColour::Custom {
        super::native_css::set_window_colors(&[(VARIABLE, look.custom)]);
    } else {
        super::native_css::set_window_colors(&[]);
    }
}

/// The look in use ([`set`]).
pub fn look() -> SelectionLook {
    LOOK.with(Cell::get)
}

/// The fill of a selected track header or clip whose element is `el`: the colour chosen in
/// Settings, else the `selection.background` the element sees (the theme's), else Suhina
/// Studio's. `None` when each keeps its own colour (**Clip's own colour**).
pub fn fill(el: &super::element::Entered) -> Option<Color32> {
    let look = look();
    match look.mode {
        SelectionColour::Own => None,
        SelectionColour::Custom => Some(look.custom),
        SelectionColour::Theme => Some(
            el.style()
                .and_then(|style| style.colour_variable(VARIABLE))
                .or_else(|| super::theme::default_color(VARIABLE.trim_start_matches("--")))
                .unwrap_or(Color32::from_rgb(0xDB, 0xDA, 0xFF)),
        ),
    }
}

/// `ink` where it reads on `fill`, else the text colour that does: what a waveform, notes or a
/// name are drawn in on a selected fill, which may be light or dark whatever the theme is.
pub fn readable(ink: Color32, fill: Color32) -> Color32 {
    if super::theme::text_contrast(ink, fill) >= 3.0 { ink } else { super::theme::text_on(fill) }
}
