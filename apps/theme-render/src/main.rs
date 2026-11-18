// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-theme-render <folder | built-in id> [--backdrop <image.png>] [--out <folder>]
//! [--scale <n>] [--scene <name> [--hover <label> | --press <label>]] [--budget]`: the theme drawn
//! in every scene, as PNGs (see the library's documentation).

use std::path::PathBuf;

use suhina_theme_render::scenes::Pointer;
use suhina_theme_render::{Subject, budget, composite, render_all, scenes};

const USAGE: &str = "usage: suhina-theme-render <theme-folder | built-in-theme-id> [--backdrop <image.png>] [--out <folder>] [--scale <n>] [--scene <name> [--hover <label> | --press <label>]] [--budget]

Draws the theme in each scene of the demo project and writes <scene>.raw.png (the window's own
pixels, with their alpha) and <scene>.png (the window over the backdrop, blurred behind a
see-through window) into the out folder (default: theme-shots). --hover and --press, with
--scene, put the pointer over the control with that label, its button held down with --press,
once the scene is driven. --budget prints the style engine's counts for each scene instead, with no GPU.";

struct Args {
    theme: String,
    backdrop: Option<PathBuf>,
    out: PathBuf,
    scale: f32,
    scene: Option<String>,
    pointer: Option<Pointer>,
    budget: bool,
}

/// What the command line asks for.
enum Command {
    Help,
    Run(Args),
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut parsed = Args {
        theme: String::new(),
        backdrop: None,
        out: "theme-shots".into(),
        scale: 1.0,
        scene: None,
        pointer: None,
        budget: false,
    };
    let mut theme = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value\n\n{USAGE}"));
        match arg.as_str() {
            "--backdrop" => parsed.backdrop = Some(value("--backdrop")?.into()),
            "--out" | "-o" => parsed.out = value("--out")?.into(),
            "--scale" => {
                parsed.scale = value("--scale")?
                    .parse()
                    .ok()
                    .filter(|s: &f32| (0.5..=4.0).contains(s))
                    .ok_or("--scale takes a number from 0.5 to 4")?;
            }
            "--scene" => {
                let name = value("--scene")?;
                let all = scenes::scenes();
                if !all.iter().any(|s| s.name == name) {
                    let names: Vec<_> = all.into_iter().map(|s| s.name).collect();
                    return Err(format!("no scene {name:?}; the scenes are {}", names.join(", ")));
                }
                parsed.scene = Some(name);
            }
            "--hover" | "--press" => {
                if parsed.pointer.is_some() {
                    return Err(format!("give one of --hover and --press, once\n\n{USAGE}"));
                }
                let label = value(&arg)?;
                parsed.pointer = Some(if arg == "--hover" { Pointer::Hover(label) } else { Pointer::Press(label) });
            }
            "--budget" => parsed.budget = true,
            "-h" | "--help" => return Ok(Command::Help),
            other if other.starts_with('-') => return Err(format!("unknown option {other}\n\n{USAGE}")),
            other if theme.is_none() => theme = Some(other.to_owned()),
            _ => return Err(USAGE.into()),
        }
    }
    parsed.theme = theme.ok_or(USAGE)?;
    // A label belongs to a scene: every other scene would fail for want of it.
    if parsed.pointer.is_some() && parsed.scene.is_none() {
        return Err(format!("--hover and --press need --scene\n\n{USAGE}"));
    }
    Ok(Command::Run(parsed))
}

fn run() -> Result<(), String> {
    let args = match parse(std::env::args().skip(1))? {
        Command::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        Command::Run(args) => args,
    };
    let folder = std::path::Path::new(&args.theme);
    let subject = if folder.is_dir() {
        let (subject, warnings) = Subject::folder(folder)?;
        for warning in warnings {
            eprintln!("warning: {warning}");
        }
        subject
    } else {
        Subject::built_in(&args.theme)
            .ok_or_else(|| format!("{}: neither a theme folder nor a built-in theme's id", args.theme))?
    };
    if args.budget {
        for scene in &scenes::select(args.scene.as_deref(), args.pointer.as_ref()) {
            match budget::count(&subject, scene) {
                Ok(counts) => println!("{}: {counts:?}", scene.name),
                Err(e) => println!("{}: not drawn: {e}", scene.name),
            }
        }
        return Ok(());
    }
    let backdrop = match &args.backdrop {
        Some(path) => image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8(),
        None => composite::default_backdrop(),
    };
    let done = render_all(&subject, &backdrop, &args.out, args.scale, args.scene.as_deref(), args.pointer.as_ref())?;
    for problem in &done.failed {
        eprintln!("not drawn: {problem}");
    }
    for scene in &done.clamped {
        eprintln!(
            "warning: drawn at a smaller scale than --scale, which the GPU's textures are too small for ({scene})"
        );
    }
    if !done.restless.is_empty() {
        eprintln!(
            "warning: the window keeps repainting every frame in {}, so it would use the processor without rest",
            done.restless.join(", ")
        );
    }
    println!("wrote {} images to {}", done.written.len(), args.out.display());
    if done.failed.is_empty() { Ok(()) } else { Err(format!("{} scenes could not be drawn", done.failed.len())) }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Args, String> {
        match parse(args.iter().map(|a| (*a).to_owned()))? {
            Command::Run(args) => Ok(args),
            Command::Help => Err("help".into()),
        }
    }

    #[test]
    fn hover_and_press_take_a_label_and_only_one_is_given() {
        let hover = parsed(&["nord", "--scene", "launcher", "--hover", "Record"]).unwrap().pointer;
        assert_eq!(hover, Some(Pointer::Hover("Record".into())));
        let press = parsed(&["nord", "--press", "Solo Drums", "--scene", "launcher"]).unwrap().pointer;
        assert_eq!(press, Some(Pointer::Press("Solo Drums".into())));
        assert_eq!(parsed(&["nord"]).unwrap().pointer, None);
        assert!(parsed(&["nord", "--scene", "launcher", "--hover", "a", "--press", "b"]).is_err());
        assert!(parsed(&["nord", "--scene", "launcher", "--hover"]).is_err());
        assert!(parsed(&["nord", "--hover", "Record"]).err().unwrap().contains("need --scene"));
    }
}
