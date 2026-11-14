// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a scene paints, as text: one line per shape of its last frame, in paint order, with its
//! place, size and colours. A change that should leave a theme's look alone leaves this listing
//! alone, so its digest, held in `tests/painted/<system>.txt`, checks "looks the same" without a GPU (#1023).
//!
//! Places and sizes are rounded to a quarter of a point, so a last-bit difference in a system's
//! `sin` or `cos` between machines does not change a listing; colours are exact.

use std::fmt::Write;

use egui::epaint::{ClippedShape, ColorMode, PathStroke, Shape, Stroke};
use egui::{Color32, Pos2, Rect};
use egui_kittest::Harness;
use suhina_ui::App;

/// A length in quarter points.
fn q(v: f32) -> i64 {
    if v.is_finite() { (f64::from(v) * 4.0).round() as i64 } else { i64::MAX }
}

fn pos(p: Pos2) -> String {
    format!("{},{}", q(p.x), q(p.y))
}

fn rect(r: Rect) -> String {
    format!("{} {}", pos(r.min), pos(r.max))
}

fn color(c: Color32) -> String {
    let [r, g, b, a] = c.to_array();
    format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
}

fn stroke(s: Stroke) -> String {
    format!("{}/{}", q(s.width), color(s.color))
}

fn path_stroke(s: &PathStroke) -> String {
    match &s.color {
        ColorMode::Solid(c) => format!("{}/{}", q(s.width), color(*c)),
        ColorMode::UV(_) => format!("{}/uv", q(s.width)),
    }
}

fn points(points: &[Pos2]) -> String {
    points.iter().map(|p| pos(*p)).collect::<Vec<_>>().join(" ")
}

/// A texture by what it holds (its name, which names its content: `css-gradient-<key>`,
/// `svg-<key>`) and size, not by its id, which depends on the order textures were made in.
fn texture(ctx: &egui::Context, id: egui::TextureId) -> String {
    match id {
        egui::TextureId::Managed(_) => ctx
            .tex_manager()
            .read()
            .meta(id)
            .map_or_else(|| "gone".to_owned(), |meta| format!("{:?}{:?}", meta.name, meta.size)),
        egui::TextureId::User(n) => format!("user{n}"),
    }
}

fn shape(out: &mut String, ctx: &egui::Context, depth: usize, shape: &Shape) {
    let pad = " ".repeat(depth);
    // `write!` to a `String` cannot fail.
    let _ = match shape {
        Shape::Noop => Ok(()),
        Shape::Vec(shapes) => {
            for s in shapes {
                self::shape(out, ctx, depth, s);
            }
            Ok(())
        }
        Shape::Circle(c) => {
            writeln!(out, "{pad}circle {} r{} {} {}", pos(c.center), q(c.radius), color(c.fill), stroke(c.stroke))
        }
        Shape::Ellipse(e) => writeln!(
            out,
            "{pad}ellipse {} r{},{} {} {}",
            pos(e.center),
            q(e.radius.x),
            q(e.radius.y),
            color(e.fill),
            stroke(e.stroke)
        ),
        Shape::LineSegment { points: [a, b], stroke: s } => {
            writeln!(out, "{pad}line {} {} {}", pos(*a), pos(*b), stroke(*s))
        }
        Shape::Path(p) => writeln!(
            out,
            "{pad}path{} {} {} [{}]",
            if p.closed { " closed" } else { "" },
            color(p.fill),
            path_stroke(&p.stroke),
            points(&p.points)
        ),
        Shape::Rect(r) => {
            let c = r.corner_radius;
            writeln!(
                out,
                "{pad}rect {} r{},{},{},{} {} {} blur{} {}",
                rect(r.rect),
                c.nw,
                c.ne,
                c.sw,
                c.se,
                color(r.fill),
                stroke(r.stroke),
                q(r.blur_width),
                r.brush.as_ref().map_or_else(String::new, |b| format!(
                    "texture {} uv {}",
                    texture(ctx, b.fill_texture_id),
                    rect(b.uv)
                ))
            )
        }
        Shape::Text(t) => {
            let job = &t.galley.job;
            let sections: Vec<String> = job
                .sections
                .iter()
                .map(|s| {
                    let text = job.text.get(s.byte_range.start.0..s.byte_range.end.0).unwrap_or("");
                    format!("{text:?} {} {}", color(s.format.color), q(s.format.font_id.size))
                })
                .collect();
            writeln!(
                out,
                "{pad}text {} {} {} {} angle{} [{}]",
                pos(t.pos),
                rect(t.galley.rect),
                color(t.fallback_color),
                color(t.override_text_color.unwrap_or(Color32::TRANSPARENT)),
                q(t.angle * 1000.0),
                sections.join(" | ")
            )
        }
        Shape::Mesh(m) => {
            let mut line = format!("{pad}mesh {} {} {}", texture(ctx, m.texture_id), m.indices.len(), m.vertices.len());
            for v in &m.vertices {
                let _ = write!(
                    line,
                    " {}:{}:{}",
                    pos(v.pos),
                    pos(Pos2::new(v.uv.x * 256.0, v.uv.y * 256.0)),
                    color(v.color)
                );
            }
            writeln!(out, "{line}")
        }
        Shape::QuadraticBezier(b) => writeln!(
            out,
            "{pad}quadratic {} {} {} [{}]",
            color(b.fill),
            path_stroke(&b.stroke),
            b.closed,
            points(&b.points)
        ),
        Shape::CubicBezier(b) => writeln!(
            out,
            "{pad}cubic {} {} {} [{}]",
            color(b.fill),
            path_stroke(&b.stroke),
            b.closed,
            points(&b.points)
        ),
        Shape::Band(b) => {
            let mut line = format!("{pad}band {} {} angle{}", color(b.fill), stroke(b.stroke), q(b.angle * 1000.0));
            for p in &b.points {
                let _ = write!(line, " {}:{},{}", q(p.x), q(p.y.min), q(p.y.max));
            }
            writeln!(out, "{line}")
        }
        Shape::Callback(c) => writeln!(out, "{pad}callback {}", rect(c.rect)),
    };
}

/// The listing of `shapes`: each clip rectangle once, then the shapes it clips.
pub fn listing(ctx: &egui::Context, shapes: &[ClippedShape]) -> String {
    let mut out = String::new();
    let mut clip = None;
    for clipped in shapes {
        if clip != Some(clipped.clip_rect) {
            clip = Some(clipped.clip_rect);
            let _ = writeln!(out, "clip {}", rect(clipped.clip_rect));
        }
        shape(&mut out, ctx, 1, &clipped.shape);
    }
    out
}

/// The listing of the window's last frame.
pub fn last_frame(h: &Harness<'static, App>) -> String {
    listing(&h.ctx, &h.output().shapes)
}

/// A 64-bit FNV-1a digest of `text`: stable between runs, machines and Rust versions.
pub fn digest(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3))
}

/// The listing of `scene` drawn in `subject`: driven, settled, then one more frame.
/// `engine` runs the style engine even when the theme has no sheet of its own (Suhina Studio,
/// whose colours are all in its palette), as an empty `user.css` does.
pub fn scene(subject: &crate::Subject, scene: &crate::scenes::Scene, engine: bool) -> Result<String, String> {
    let mut h = crate::headless((scene.app)()?, subject, scene.width);
    suhina_ui::kit::native_css::set_user_sheet(engine.then(|| "".into()));
    let driven = (scene.drive)(&mut h);
    let _steps = h.try_run();
    h.step();
    let listing = last_frame(&h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    driven.map(|()| listing)
}
