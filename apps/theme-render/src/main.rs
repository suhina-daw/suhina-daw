// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-theme-render <folder | built-in id> [--backdrop <image.png>] [--out <folder>]
//! [--scale <n>] [--scene <name>] [--budget]`: the theme drawn in every scene, as PNGs (see the
//! library's documentation).

use std::path::PathBuf;

use suhina_theme_render::{Subject, budget, composite, render_all, scenes};

const USAGE: &str = "usage: suhina-theme-render <theme-folder | built-in-theme-id> [--backdrop <image.png>] [--out <folder>] [--scale <n>] [--scene <name>] [--budget]

Draws the theme in each scene of the demo project and writes <scene>.raw.png (the window's own
pixels, with their alpha) and <scene>.png (the window over the backdrop, blurred behind a
see-through window) into the out folder (default: theme-shots). --budget prints the style
engine's counts for each scene instead, with no GPU.";

struct Args {
    theme: String,
    backdrop: Option<PathBuf>,
    out: PathBuf,
    scale: f32,
    scene: Option<String>,
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
            "--budget" => parsed.budget = true,
            "-h" | "--help" => return Ok(Command::Help),
            other if other.starts_with('-') => return Err(format!("unknown option {other}\n\n{USAGE}")),
            other if theme.is_none() => theme = Some(other.to_owned()),
            _ => return Err(USAGE.into()),
        }
    }
    parsed.theme = theme.ok_or(USAGE)?;
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
        for scene in &scenes::select(args.scene.as_deref()) {
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
    let done = render_all(&subject, &backdrop, &args.out, args.scale, args.scene.as_deref())?;
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
