// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the leaf style cache saves on a large arrangement and a dense piano roll:
//! `cargo bench -p suhina-style --bench style_cache`.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a measurement on fixed inputs, not app code
use std::time::{Duration, Instant};
use suhina_style::{
    cascade::{self, AppliedSheet, Layer},
    media::Viewport,
    resolver::{Resolver, SheetScope},
    selector::{Element, Tree},
    sheet::Sheet,
};

const SHEET: &str = "
    :root { --background: #16181c; --line: #2a2e35; --accent: #79e2bd; }
    window { color: #e8e8e8; background-color: var(--background); }
    track-lane { --track: attr(color type(<color>)); background-color: color-mix(in oklab, var(--track), var(--background) 90%); }
    track-lane:selected { background-color: color-mix(in oklab, var(--track), var(--background) 80%); }
    clip { background-color: color-mix(in oklab, attr(color type(<color>)), black 30%);
           border-color: attr(color type(<color>)); color: contrast-color(attr(color type(<color>))); radius: 3px; }
    clip:selected { border-color: white; box-shadow: 0 0 0 1px var(--accent); }
    clip:playing { opacity: 0.9; }
    clip:muted { opacity: 0.4; background-color: color-mix(in srgb, attr(color type(<color>)), #808080 70%); }
    clip[kind=audio] { radius: 2px; }
    piano-roll { --clip: attr(color type(<color>)); background-color: #101114; }
    note { background-color: var(--clip); border-color: color-mix(in oklab, var(--clip), black 40%); radius: 2px; }
    note:selected { background-color: color-mix(in oklab, var(--clip), white 35%); border-color: var(--accent); }
    note:playing { box-shadow: 0 0 4px var(--clip); }
    note:muted { opacity: 0.35; }
    note[kind=ghost] { opacity: 0.2; }
";

const PALETTE: [&str; 16] = [
    "#e8913a", "#e8c53a", "#a3e83a", "#3ae86b", "#3ae8c5", "#3aa3e8", "#3a52e8", "#913ae8", "#e83ac5", "#e83a6b",
    "#c0c0c0", "#8a6d4a", "#4a8a6d", "#6d4a8a", "#8a4a4a", "#4a6d8a",
];

fn viewport() -> Viewport {
    Viewport { width: 1600.0, height: 1000.0, dark: true, reduced_motion: false }
}

fn time(label: &str, frames: u32, mut frame: impl FnMut()) -> Duration {
    let start = Instant::now();
    for _ in 0..frames {
        frame();
    }
    let each = start.elapsed() / frames;
    println!("{label:<58} {:>10.3} ms", each.as_secs_f64() * 1000.0);
    each
}

/// 64 tracks of 120 clips each: most clips in their track's colour, one in eight its own.
fn arrangement() {
    let tracks = 64;
    let mut elements = vec![Element::new("window", None), Element::new("arrangement", Some(0))];
    for track in 0..tracks {
        elements.push(Element::new("track-lane", Some(1)).with_attribute("color", PALETTE[track % 16]));
    }
    let tree = Tree::new(elements).unwrap();
    let clips: Vec<(usize, Element)> = (0..tracks * 120)
        .map(|index| {
            let track = index / 120;
            let colour = if index % 8 == 0 {
                format!("#{:06x}", index * 2_654_435 % 0xff_ffff)
            } else {
                PALETTE[track % 16].to_owned()
            };
            let mut clip = Element::new("clip", None)
                .with_attribute("color", &colour)
                .with_attribute("kind", if index % 3 == 0 { "audio" } else { "midi" });
            if index % 20 == 0 {
                clip.states.push("selected".into());
            }
            if index % 50 == 0 {
                clip.states.push("muted".into());
            }
            (2 + track, clip)
        })
        .collect();
    let mut resolver =
        Resolver::new(vec![SheetScope { sheet: Sheet::parse(SHEET).unwrap(), layer: Layer::Theme, root: None }])
            .unwrap();
    println!("Arrangement: {tracks} tracks, {} clips", clips.len());
    time("  first frame (empty cache)", 1, || {
        resolver.resolve(&tree, viewport()).unwrap();
        for (parent, clip) in &clips {
            resolver.leaf_style(*parent, clip).unwrap();
        }
    });
    let stats = resolver.leaf_stats();
    time("  next frames (cache kept)", 100, || {
        resolver.resolve(&tree, viewport()).unwrap();
        for (parent, clip) in &clips {
            resolver.leaf_style(*parent, clip).unwrap();
        }
    });
    println!("  {} styles computed for {} clips", stats.entries, clips.len());
    // The pointer moves over one track and off it again, each frame.
    let mut hovered = tree.elements().to_vec();
    hovered[12].states.push("hover".into());
    let hovered = Tree::new(hovered).unwrap();
    let mut frame = 0;
    time("  frames hovering one track on and off", 100, || {
        frame += 1;
        resolver.resolve(if frame % 2 == 0 { &tree } else { &hovered }, viewport()).unwrap();
        for (parent, clip) in &clips {
            resolver.leaf_style(*parent, clip).unwrap();
        }
    });
}

/// 20,000 notes of one clip, some selected, playing, muted or ghost notes.
fn piano_roll() {
    let tree = Tree::new(vec![
        Element::new("window", None),
        Element::new("piano-roll", Some(0)).with_attribute("color", "#3aa3e8"),
    ])
    .unwrap();
    let notes: Vec<Element> = (0..20_000)
        .map(|index| {
            let mut note = Element::new("note", None);
            if index % 10 == 0 {
                note.states.push("selected".into());
            }
            if index % 37 == 0 {
                note.states.push("playing".into());
            }
            if index % 101 == 0 {
                note.states.push("muted".into());
            }
            if index % 13 == 0 {
                note = note.with_attribute("kind", "ghost");
            }
            note
        })
        .collect();
    let sheet = Sheet::parse(SHEET).unwrap();
    let mut resolver =
        Resolver::new(vec![SheetScope { sheet: sheet.clone(), layer: Layer::Theme, root: None }]).unwrap();
    println!("Piano roll: {} notes", notes.len());
    time("  first frame (empty cache)", 1, || {
        resolver.resolve(&tree, viewport()).unwrap();
        for note in &notes {
            resolver.leaf_style(1, note).unwrap();
        }
    });
    let stats = resolver.leaf_stats();
    time("  next frames (cache kept)", 100, || {
        resolver.resolve(&tree, viewport()).unwrap();
        for note in &notes {
            resolver.leaf_style(1, note).unwrap();
        }
    });
    println!("  {} styles computed for {} notes", stats.entries, notes.len());
    // Without a cache, each note is an element of the tree. A tree holds at most 4096 elements,
    // so this styles 4000 notes and scales the time to all of them.
    let mut elements = tree.elements().to_vec();
    elements.extend(notes.iter().take(4000).map(|note| Element { parent: Some(1), ..note.clone() }));
    let whole = Tree::new(elements).unwrap();
    let applied = [AppliedSheet { sheet: &sheet, layer: Layer::Theme, root: None }];
    let each = time("  4000 notes as tree elements, no cache", 5, || {
        cascade::compute(&whole, viewport(), &applied).unwrap();
    });
    println!(
        "{:<58} {:>10.3} ms",
        "  (scaled to every note)",
        each.as_secs_f64() * 1000.0 * notes.len() as f64 / 4000.0
    );
}

fn main() {
    arrangement();
    piano_roll();
}
