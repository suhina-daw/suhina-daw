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
    /// Settings › Appearance › Background image: a user-owned PNG; no desktop capture.
    BackgroundImage,
    /// Settings › Projects and backups › Projects folder.
    ProjectsFolder,
    /// Settings › Projects and backups › Templates folder.
    TemplatesFolder,
    /// Settings › Projects and backups › Startup template.
    StartTemplate,
    /// The folder of the save prompt (File › Save as…).
    SaveFolder,
    /// The audio file an export writes (File › Export…).
    ExportFile,
    /// A folder the person allows attached MCP agents to write in.
    McpWriteFolder,
    /// A folder to look in for plug-ins (Settings › Plug-ins, added with nothing typed).
    PluginFolder,
    /// A project to open (File › Open…, the start panel's Open…).
    OpenProject,
    /// A folder for the browser (+ Add folder).
    BrowserFolder,
    /// Amp models folder (docs/spec/nam-amp-and-cab.md).
    NamModelsFolder,
    /// Settings › Themes › Themes folder.
    ThemesFolder,
    /// Settings › Projects and backups › Settings folder.
    DataFolder,
    /// Cabinet sounds folder (docs/spec/nam-amp-and-cab.md).
    IrFolder,
    /// The folder Copy to folder… copies samples into (`library.copyTo`, #865).
    CopyToFolder,
}

impl PickField {
    /// Whether a folder is picked rather than a file.
    pub fn folder(self) -> bool {
        !matches!(self, Self::StartTemplate | Self::OpenProject | Self::ExportFile | Self::BackgroundImage)
    }

    /// The dialog's title.
    pub fn title(self) -> &'static str {
        match self {
            Self::BackgroundImage => "Background image (PNG)",
            Self::ProjectsFolder => "Projects folder",
            Self::TemplatesFolder => "Templates folder",
            Self::StartTemplate => "Template to start from",
            Self::SaveFolder => "Folder to save in",
            Self::ExportFile => "Export audio to",
            Self::McpWriteFolder => "Allow MCP writes in",
            Self::PluginFolder => "Add a plug-in folder",
            Self::OpenProject => "Open a project",
            Self::BrowserFolder => "Add a folder to the browser",
            Self::NamModelsFolder => "Amp models folder",
            Self::ThemesFolder => "Themes folder",
            Self::DataFolder => "Settings folder",
            Self::IrFolder => "Cabinet sounds folder",
            Self::CopyToFolder => "Copy the samples to",
        }
    }

    /// The file extensions a file dialog offers; none for a folder.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::ExportFile => &["wav", "flac", "ogg", "mp3"],
            Self::BackgroundImage => &["png"],
            field if field.folder() => &[],
            _ => &["suhina", "als"],
        }
    }

    /// The name the file dialog's filter carries.
    pub fn filter_label(self) -> &'static str {
        match self {
            Self::ExportFile => "Audio",
            Self::BackgroundImage => "PNG image",
            _ => "Projects",
        }
    }
}

/// A dialog the window asks the app to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickRequest {
    pub field: PickField,
    /// The folder the dialog opens in, when the field names one.
    pub start: Option<PathBuf>,
}
