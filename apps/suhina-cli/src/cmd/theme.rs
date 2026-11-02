// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-cli theme …`: theme files.

use crate::*;

/// `fm-synth voices <file.syx>`.
/// `theme pack` and `theme validate` (`docs/spec/theme-skins.md` › Theme packages).
pub(crate) fn theme(args: &[String]) -> Result<(), String> {
    use suhina_theme_package as package;
    let usage = || {
        format!(
            "usage: suhina-cli theme (pack <theme-folder> [-o <out.theme.suhina>] | validate (<file.theme.suhina> | <folder>))\n\n{USAGE}"
        )
    };
    let print_warnings = |warnings: &[String]| {
        for warning in warnings {
            println!("warning: {warning}");
        }
    };
    match (args.first().map(String::as_str), args.get(1)) {
        (Some("pack"), Some(folder)) => {
            let folder = std::path::Path::new(folder);
            let out = match (args.get(2).map(String::as_str), args.get(3)) {
                (Some("-o" | "--output"), Some(out)) => std::path::PathBuf::from(out),
                (None, _) => {
                    let name = folder.file_name().and_then(|n| n.to_str()).ok_or("the folder has no name")?;
                    folder.with_file_name(format!("{name}{}", package::EXTENSION))
                }
                _ => return Err(usage()),
            };
            let id = folder
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_ascii_lowercase().replace(' ', "-"))
                .ok_or("the folder has no name")?;
            let files = package::read_folder(folder)?;
            let name = files
                .get("theme.css")
                .and_then(|css| std::str::from_utf8(css).ok())
                .and_then(package::sheet_name)
                .unwrap_or_else(|| id.clone());
            let warnings = package::validate(&files)?;
            let bytes = package::pack(&id, &name, &files)?;
            std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
            print_warnings(&warnings);
            println!("packed {name} ({} files) into {}", files.len(), out.display());
            Ok(())
        }
        (Some("validate"), Some(path)) => {
            let path = std::path::Path::new(path);
            let (what, warnings) = if path.is_dir() {
                let files = package::read_folder(path)?;
                (format!("{} files", files.len()), package::validate(&files)?)
            } else {
                let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let read = package::read(&bytes)?;
                (format!("{} ({}), {} files", read.name, read.id, read.files.len()), read.warnings)
            };
            print_warnings(&warnings);
            println!("valid: {what}, {} warnings", warnings.len());
            Ok(())
        }
        _ => Err(usage()),
    }
}
