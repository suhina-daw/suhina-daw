// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-theme-render <folder | built-in id> [--backdrop <image.png>] [--out <folder>]
//! [--scale <n>] [--scene <name> [--hover <label> | --press <label>]] [--outline]
//! [--budget | --coverage | --probe <x,y>]`: the theme drawn in every scene, as PNGs (see the
//! library's documentation), or what the style engine did in them.

use std::path::PathBuf;

use suhina_theme_render::scenes::Pointer;
use suhina_theme_render::{Subject, budget, composite, inspect, render_all, scenes};

const USAGE: &str = "usage: suhina-theme-render <theme-folder | built-in-theme-id> [--backdrop <image.png>] [--out <folder>] [--scale <n>] [--scene <name> [--hover <label> | --press <label>]] [--outline] [--budget | --coverage | --probe <x,y>]

Draws the theme in each scene of the demo project and writes <scene>.raw.png (the window's own
pixels, with their alpha) and <scene>.png (the window over the backdrop, blurred behind a
see-through window) into the out folder (default: theme-shots). --hover and --press, with
--scene, put the pointer over the control with that label, its button held down with --press,
once the scene is driven. --outline also writes <scene>.outline.png, with every element's box and
name drawn over the window.

Instead of drawing, with no GPU:
  --budget     prints the style engine's counts for each scene.
  --coverage   lists the rules of the theme's own sheet that matched no element in any scene.
  --probe x,y  with --scene: the element at that point of the window (in points, from its top
               left corner; most scenes' window is 1280 by 900), the elements it is in, and every
               rule that matches it, the strongest first, with what became of each declaration.";

struct Args {
    theme: String,
    backdrop: Option<PathBuf>,
    out: PathBuf,
    scale: f32,
    scene: Option<String>,
    pointer: Option<Pointer>,
    outline: bool,
    mode: Mode,
}

/// What is done with the scenes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Draw,
    Budget,
    Coverage,
    Probe(egui::Pos2),
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
        outline: false,
        mode: Mode::Draw,
    };
    let set_mode = |parsed: &mut Args, mode: Mode| {
        if parsed.mode != Mode::Draw {
            return Err(format!("give one of --budget, --coverage and --probe\n\n{USAGE}"));
        }
        parsed.mode = mode;
        Ok(())
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
                // An option here means the label is missing.
                if label.starts_with('-') {
                    return Err(format!("{arg} takes the label of a control, not {label}\n\n{USAGE}"));
                }
                parsed.pointer = Some(if arg == "--hover" { Pointer::Hover(label) } else { Pointer::Press(label) });
            }
            "--budget" => set_mode(&mut parsed, Mode::Budget)?,
            "--coverage" => set_mode(&mut parsed, Mode::Coverage)?,
            "--probe" => {
                let at = value("--probe")?;
                let point = at
                    .split_once(',')
                    .and_then(|(x, y)| Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?)))
                    .filter(|p| p.x.is_finite() && p.y.is_finite())
                    .ok_or_else(|| format!("--probe takes a point as x,y in points, not {at}"))?;
                set_mode(&mut parsed, Mode::Probe(point))?;
            }
            "--outline" => parsed.outline = true,
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
    if matches!(parsed.mode, Mode::Probe(_)) && parsed.scene.is_none() {
        return Err(format!("--probe needs --scene: a point is in one scene\n\n{USAGE}"));
    }
    if parsed.outline && parsed.mode != Mode::Draw {
        return Err(format!(
            "--outline draws over the pictures, which --budget, --coverage and --probe do not draw\n\n{USAGE}"
        ));
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
    match args.mode {
        Mode::Draw | Mode::Budget => {}
        Mode::Coverage => {
            let scenes = scenes::select(args.scene.as_deref(), args.pointer.as_ref());
            let found = inspect::coverage(&subject, &scenes)?;
            for problem in &found.failed {
                eprintln!("not driven: {problem}");
            }
            let driven = found.driven;
            let (artwork, unused): (Vec<_>, Vec<_>) = found.unused.into_iter().partition(|rule| rule.artwork.is_some());
            if !artwork.is_empty() {
                println!(
                    "Artwork rules no element matched: Suhina reads their image when it loads the theme, but \
                     their other properties did nothing in these scenes:"
                );
                for rule in &artwork {
                    match rule.artwork.as_deref().filter(|now| *now != rule.selector) {
                        Some(now) => println!("  line {}: {} (an earlier form of {now})", rule.line, rule.selector),
                        None => println!("  line {}: {}", rule.line, rule.selector),
                    }
                }
            }
            if unused.is_empty() {
                println!("Every other rule of the theme's sheet matched an element in {driven} scenes.");
            } else {
                println!("Rules of the theme's sheet that matched no element in {driven} scenes:");
                for rule in &unused {
                    println!("  line {}: {}", rule.line, rule.selector);
                }
                println!(
                    "A rule for a state no scene shows, or in an @media block for another window size or the \
                     other color scheme, cannot match here, and is listed too."
                );
            }
            return if found.failed.is_empty() {
                Ok(())
            } else {
                Err(format!("{} scenes not counted", found.failed.len()))
            };
        }
        Mode::Probe(at) => {
            let scene = scenes::select(args.scene.as_deref(), args.pointer.as_ref())
                .into_iter()
                .next()
                .ok_or("no such scene")?;
            let h = inspect::settled(&subject, &scene)?;
            print!("{}", inspect::probe(&h, at)?);
            return Ok(());
        }
    }
    if args.mode == Mode::Budget {
        let mut failed = 0;
        for scene in &scenes::select(args.scene.as_deref(), args.pointer.as_ref()) {
            match budget::count(&subject, scene) {
                Ok(counts) => println!("{}: {counts:?}", scene.name),
                Err(e) => {
                    failed += 1;
                    println!("{}: not drawn: {e}", scene.name);
                }
            }
        }
        // As for the pictures: a scene left undrawn, a refused label included, is a failure.
        return if failed == 0 { Ok(()) } else { Err(format!("{failed} scenes not drawn")) };
    }
    let backdrop = match &args.backdrop {
        Some(path) => image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8(),
        None => composite::default_backdrop(),
    };
    let done = render_all(
        &subject,
        &backdrop,
        &args.out,
        args.scale,
        args.scene.as_deref(),
        args.pointer.as_ref(),
        args.outline,
    )?;
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
        assert!(
            parsed(&["nord", "--scene", "launcher", "--hover", "--budget"]).err().unwrap().contains("takes the label")
        );
        assert!(parsed(&["nord", "--hover", "Record"]).err().unwrap().contains("need --scene"));
    }

    #[test]
    fn one_mode_is_given_and_a_probe_takes_a_point_in_a_scene() {
        assert_eq!(parsed(&["nord"]).unwrap().mode, Mode::Draw);
        assert_eq!(parsed(&["nord", "--coverage"]).unwrap().mode, Mode::Coverage);
        assert_eq!(
            parsed(&["nord", "--scene", "launcher", "--probe", "12.5, 30"]).unwrap().mode,
            Mode::Probe(egui::pos2(12.5, 30.0))
        );
        assert!(parsed(&["nord", "--probe", "1,2"]).err().unwrap().contains("needs --scene"));
        for bad in ["1", "a,b", "1;2", "inf,2"] {
            let error = parsed(&["nord", "--scene", "launcher", "--probe", bad]).err().unwrap();
            assert!(error.contains("takes a point"), "{bad}: {error}");
        }
        assert!(parsed(&["nord", "--budget", "--coverage"]).err().unwrap().contains("give one of"));
        assert!(parsed(&["nord", "--outline"]).unwrap().outline);
        assert!(parsed(&["nord", "--outline", "--coverage"]).err().unwrap().contains("--outline draws"));
    }
}
