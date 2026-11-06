// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-cli theme …`: theme files.

use crate::*;

/// `theme pack` and `theme validate` (`docs/spec/theme-skins.md` › Theme packages), and
/// `theme render`, which runs `suhina-theme-render`.
pub(crate) fn theme(args: &[String]) -> Result<(), String> {
    use suhina_theme_package as package;
    let usage = || {
        format!(
            "usage: suhina-cli theme (pack <theme-folder> [-o <out.theme.suhina>] | validate (<file.theme.suhina> | <folder>) | render <theme-folder | built-in-theme-id> [--backdrop <image.png>] [--out <folder>] [--scale <n>] [--scene <name>] [--budget])\n\n{USAGE}"
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
            let name = package::theme_name(&files).unwrap_or_else(|| id.clone());
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
        (Some("render"), Some(_)) => render(&args[1..]),
        _ => Err(usage()),
    }
}

/// `theme render …`: `suhina-theme-render` from beside this program, with the arguments passed
/// on, so this program needs no GPU or window toolkit of its own.
fn render(args: &[String]) -> Result<(), String> {
    let name = format!("suhina-theme-render{}", std::env::consts::EXE_SUFFIX);
    let program = std::env::current_exe()
        .ok()
        .map(|exe| exe.canonicalize().unwrap_or(exe))
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|path| path.is_file())
        .ok_or_else(|| format!("{name} is not beside suhina-cli; build it with cargo build -p suhina-theme-render"))?;
    let status =
        std::process::Command::new(&program).args(args).status().map_err(|e| format!("{}: {e}", program.display()))?;
    if status.success() {
        return Ok(());
    }
    // The program has said why; this passes its exit code on without a second message.
    std::process::exit(status.code().unwrap_or(1));
}
