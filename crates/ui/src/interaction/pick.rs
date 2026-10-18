// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Paths chosen with the system's own file and folder dialogs. The window only asks, with a
//! [`PickRequest`] the app takes ([`crate::App::take_pick_request`]); the desktop app shows the
//! dialog and hands back what was chosen ([`crate::App::set_picked`]). So the window, and its
//! tests, need no dialog, and every path can still be typed.

use std::path::PathBuf;

/// The field a chosen path goes into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickField {
    /// Settings › General › Projects folder.
    ProjectsFolder,
    /// Settings › General › Templates folder.
    TemplatesFolder,
    /// Settings › General › Start template.
    StartTemplate,
    /// The folder of the save prompt (File › Save as…).
    SaveFolder,
    /// A project to open (File › Open…, the start panel's Open…).
    OpenProject,
    /// A folder for the browser (+ Add folder).
    BrowserFolder,
    /// Settings › General › NAM models folder (docs/spec/nam-amp-and-cab.md).
    NamModelsFolder,
    /// Settings › General › IR folder (docs/spec/nam-amp-and-cab.md).
    IrFolder,
}

impl PickField {
    /// Whether a folder is picked rather than a file.
    pub fn folder(self) -> bool {
        !matches!(self, Self::StartTemplate | Self::OpenProject)
    }

    /// The dialog's title.
    pub fn title(self) -> &'static str {
        match self {
            Self::ProjectsFolder => "Projects folder",
            Self::TemplatesFolder => "Templates folder",
            Self::StartTemplate => "Template to start from",
            Self::SaveFolder => "Folder to save in",
            Self::OpenProject => "Open a project",
            Self::BrowserFolder => "Add a folder to the browser",
            Self::NamModelsFolder => "NAM models folder",
            Self::IrFolder => "IR folder",
        }
    }

    /// The file extensions a file dialog offers; none for a folder.
    pub fn extensions(self) -> &'static [&'static str] {
        if self.folder() { &[] } else { &["suhina", "als"] }
    }
}

/// A dialog the window asks the app to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickRequest {
    pub field: PickField,
    /// The folder the dialog opens in, when the field names one.
    pub start: Option<PathBuf>,
}
