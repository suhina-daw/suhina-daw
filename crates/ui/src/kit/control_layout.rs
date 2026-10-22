// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme-owned native control placement. Paint and interaction use the same rectangle.
use egui::{Rect, Vec2};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

pub type Placements = BTreeMap<String, Placement>;
pub const MAX_CONTROLS: usize = 256;

/// Keep public authoring names readable and stable, including SDK ids containing Unicode.
pub fn semantic_name(prefix: &str, id: &str) -> String {
    let prefix: String =
        prefix.chars().take(16).map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let direct = format!("{prefix}-{id}");
    if valid_name(&direct) {
        return direct;
    }
    let mut hash = 0xcbf29ce484222325u64;
    for byte in id.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    let slug: String =
        id.chars().take(24).map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    format!("{prefix}-{slug}-{hash:016x}")
}

#[derive(Clone)]
pub struct Entry {
    pub name: String,
    pub native: Rect,
    pub at: Rect,
    pub bounds: Rect,
}

struct Runtime {
    ctx: egui::Context,
    entries: Vec<Entry>,
}
thread_local! { static RUNTIME: RefCell<Option<Runtime>> = const { RefCell::new(None) }; }

fn draft_id() -> egui::Id {
    egui::Id::new("native-control-layout-draft")
}
pub fn editing(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<Arc<Placements>>(draft_id())).is_some()
}
pub fn draft(ctx: &egui::Context, placements: Option<Placements>) {
    ctx.data_mut(|data| {
        if let Some(placements) = placements {
            data.insert_temp(draft_id(), Arc::new(placements));
        } else {
            data.remove::<Arc<Placements>>(draft_id());
        }
    });
}

/// One native frame's measured controls. The temporary context never survives the frame.
pub struct Capture(Option<Runtime>);
impl Capture {
    pub fn begin(ctx: &egui::Context) -> Self {
        Self(RUNTIME.with(|runtime| runtime.replace(Some(Runtime { ctx: ctx.clone(), entries: Vec::new() }))))
    }
    pub fn entries(&self) -> Vec<Entry> {
        RUNTIME.with(|runtime| runtime.borrow().as_ref().map_or_else(Vec::new, |runtime| runtime.entries.clone()))
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        RUNTIME.with(|runtime| runtime.replace(self.0.take()));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    /// Fractional offset in 1/1024 logical pixels, retained by CSS/JSON round trips.
    pub subpixel_x: i16,
    pub subpixel_y: i16,
    pub bottom: Option<i32>,
    pub right: Option<i32>,
    pub hidden: bool,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub full_height: bool,
    pub full_width: bool,
    pub direction: Direction,
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

impl Placement {
    pub fn offset(self) -> Vec2 {
        Vec2::new(
            self.x as f32 + f32::from(self.subpixel_x) / 1024.0,
            self.y as f32 + f32::from(self.subpixel_y) / 1024.0,
        )
    }
    pub fn set_offset(&mut self, offset: Vec2) {
        let split = |v: f32| {
            let v = if v.is_finite() { v.clamp(-16384.0, 16384.0) } else { 0.0 };
            let units = (v * 1024.0).round() as i32;
            (units / 1024, (units % 1024) as i16)
        };
        (self.x, self.subpixel_x) = split(offset.x);
        (self.y, self.subpixel_y) = split(offset.y);
    }

    /// Explicit relative placement, in logical points; no stylesheet can escape its native panel.
    pub fn apply(self, native: Rect, bounds: Rect) -> Rect {
        if !native.is_finite() || !bounds.is_finite() || !bounds.is_positive() {
            return native;
        }
        let mut size =
            Vec2::new(self.width.map_or(native.width(), f32::from), self.height.map_or(native.height(), f32::from))
                .max(Vec2::splat(1.0))
                .min(bounds.size());
        let mut anchor = native.min + self.offset();
        if self.full_width {
            let left = self.offset().x.clamp(0.0, bounds.width());
            let right = (self.right.unwrap_or(0) as f32).clamp(0.0, bounds.width() - left);
            anchor.x = bounds.left() + left;
            size.x = (bounds.width() - left - right).max(0.0);
        } else if let Some(right) = self.right {
            anchor.x = bounds.right() - size.x - right as f32;
        }
        if self.full_height {
            let top = self.offset().y.clamp(0.0, bounds.height());
            let bottom = (self.bottom.unwrap_or(0) as f32).clamp(0.0, bounds.height() - top);
            anchor.y = bounds.top() + top;
            size.y = (bounds.height() - top - bottom).max(0.0);
        } else if let Some(bottom) = self.bottom {
            anchor.y = bounds.bottom() - size.y - bottom as f32;
        }
        let min = anchor.max(bounds.min).min(bounds.max - size);
        Rect::from_min_size(min, size)
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), &'static str> {
        match key {
            "left" | "top" | "bottom" | "right" => {
                if matches!(key, "bottom" | "right") && value == "auto" {
                    if key == "bottom" {
                        self.bottom = None;
                    } else {
                        self.right = None;
                    }
                    return Ok(());
                }
                if !matches!(key, "bottom" | "right") {
                    let n = value
                        .strip_suffix("px")
                        .and_then(|v| v.parse::<f32>().ok())
                        .filter(|v| v.is_finite() && (-16384.0..=16384.0).contains(v))
                        .ok_or("offset must be finite and within -16384..16384px")?;
                    let mut offset = self.offset();
                    if key == "left" {
                        offset.x = n;
                    } else {
                        offset.y = n;
                    }
                    self.set_offset(offset);
                    return Ok(());
                }
                let n = value
                    .strip_suffix("px")
                    .and_then(|v| v.parse::<i32>().ok())
                    .filter(|v| (-16384..=16384).contains(v))
                    .ok_or("offset must be -16384..16384px")?;
                match key {
                    "left" => self.x = n,
                    "top" => self.y = n,
                    "bottom" => self.bottom = Some(n),
                    _ => self.right = Some(n),
                }
            }
            "width" | "height" => {
                if key == "width" && value == "100%" {
                    self.full_width = true;
                    self.width = None;
                    return Ok(());
                }
                if key == "height" && value == "100%" {
                    self.full_height = true;
                    self.height = None;
                    return Ok(());
                }
                let n = if value == "auto" {
                    None
                } else {
                    Some(
                        value
                            .strip_suffix("px")
                            .and_then(|v| v.parse::<u16>().ok())
                            .filter(|v| (1..=4096).contains(v))
                            .ok_or("size must be auto or 1..4096px")?,
                    )
                };
                if key == "width" {
                    self.width = n;
                    self.full_width = false;
                } else {
                    self.height = n;
                    self.full_height = false;
                }
            }
            "--suhina-direction" => {
                self.direction = match value {
                    "vertical" => Direction::Vertical,
                    "horizontal" => Direction::Horizontal,
                    _ => return Err("direction must be vertical or horizontal"),
                }
            }
            "display" => {
                self.hidden = match value {
                    "none" => true,
                    "block" => false,
                    _ => return Err("display must be none or block"),
                }
            }
            _ => return Err("unsupported native control placement property"),
        }
        Ok(())
    }

    pub fn to_json(self) -> Value {
        json!({"left": self.offset().x, "top": self.offset().y, "bottom": self.bottom, "right": self.right, "hidden": self.hidden, "width": if self.full_width { json!("100%") } else { json!(self.width) }, "height": if self.full_height { json!("100%") } else { json!(self.height) },
            "direction": if self.direction == Direction::Horizontal { "horizontal" } else { "vertical" }})
    }

    pub fn from_json(value: &Value, warnings: &mut Vec<String>) -> Self {
        let mut result = Self::default();
        let Some(properties) = value.as_object().filter(|p| p.len() <= 8) else {
            warnings.push("control placement must be an object of at most eight properties".into());
            return result;
        };
        for (key, value) in properties {
            if matches!(key.as_str(), "width" | "height" | "bottom" | "right") && value.is_null() {
                continue;
            }
            if key == "hidden" {
                if let Some(hidden) = value.as_bool() {
                    result.hidden = hidden;
                } else {
                    warnings.push("control hidden must be boolean".into());
                }
                continue;
            }
            let (key, text) = if matches!(key.as_str(), "width" | "height") && value.as_str() == Some("100%") {
                (key.as_str(), "100%".to_owned())
            } else if key == "direction" {
                ("--suhina-direction", value.as_str().unwrap_or("").to_owned())
            } else {
                (key.as_str(), value.as_f64().map_or(String::new(), |v| format!("{v}px")))
            };
            if let Err(error) = result.set(key, &text) {
                warnings.push(format!("control {key}: {error}"));
            }
        }
        result
    }
}

pub fn placement(name: &str) -> Placement {
    if let Some(placement) = RUNTIME.with(|runtime| {
        runtime.borrow().as_ref().and_then(|runtime| {
            runtime
                .ctx
                .data(|data| data.get_temp::<Arc<Placements>>(draft_id()))
                .map(|draft| draft.get(name).copied().unwrap_or_default())
        })
    }) {
        return placement;
    }
    super::skin::current().controls.get(name).copied().unwrap_or_default()
}

pub fn place(name: &str, native: Rect, bounds: Rect) -> Rect {
    let at = resolve(name, native, bounds);
    register(name, native, at, bounds);
    at
}

/// Resolve geometry without registering an edit handle for a part a renderer does not draw.
pub fn resolve(name: &str, native: Rect, bounds: Rect) -> Rect {
    let layout = placement(name);
    if layout == Placement::default() { native } else { layout.apply(native, bounds) }
}

fn register(name: &str, native: Rect, at: Rect, bounds: Rect) {
    RUNTIME.with(|runtime| {
        if let Some(runtime) = runtime.borrow_mut().as_mut()
            && runtime.entries.len() < 4096
            && runtime.ctx.data(|data| data.get_temp::<Arc<Placements>>(draft_id())).is_some()
            && native.is_finite()
            && at.is_finite()
            && at.is_positive()
        {
            runtime.entries.push(Entry { name: name.into(), native, at, bounds });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_width_uses_left_and_right_insets_and_round_trips() {
        let mut p = Placement::default();
        p.set("width", "100%").unwrap();
        p.set("left", "4.5px").unwrap();
        p.set("right", "6px").unwrap();
        let bounds = Rect::from_min_size(egui::pos2(10.0, 20.0), Vec2::new(100.0, 80.0));
        let native = Rect::from_min_size(bounds.min + Vec2::splat(12.0), Vec2::splat(20.0));
        let at = p.apply(native, bounds);
        assert_eq!(at.left(), 14.5);
        assert_eq!(at.right(), 104.0);
        assert_eq!(at.height(), 20.0);
        let mut warnings = Vec::new();
        assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
        assert!(warnings.is_empty());
        p.set("width", "20px").unwrap();
        assert!(!p.full_width);
    }

    #[test]
    fn fractional_offsets_survive_storage_and_reject_nonfinite_input() {
        let mut p = Placement::default();
        p.set("left", "1.25px").unwrap();
        p.set("top", "-2.125px").unwrap();
        assert_eq!(p.offset(), Vec2::new(1.25, -2.125));
        let mut warnings = Vec::new();
        assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
        assert!(warnings.is_empty());
        assert!(p.set("left", "NaNpx").is_err());
        assert!(p.set("top", "infpx").is_err());
    }

    #[test]
    fn full_height_tracks_panel_edges_and_round_trips() {
        let mut p = Placement::default();
        p.set("height", "100%").unwrap();
        p.set("top", "4px").unwrap();
        p.set("bottom", "6px").unwrap();
        for height in [30.0, 200.0] {
            let bounds = Rect::from_min_size(egui::pos2(10.0, -40.0), Vec2::new(100.0, height));
            let native = Rect::from_min_size(bounds.min + Vec2::new(80.0, 8.0), Vec2::new(10.0, 12.0));
            let at = p.apply(native, bounds);
            assert_eq!(at.top(), bounds.top() + 4.0);
            assert_eq!(at.bottom(), bounds.bottom() - 6.0);
            assert_eq!(at.width(), 10.0);
        }
        let mut warnings = Vec::new();
        assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
        assert!(warnings.is_empty());
        p.set("height", "auto").unwrap();
        assert!(!p.full_height);
        assert!(p.set("height", "200%").is_err());
    }

    #[test]
    fn semantic_names_are_stable_bounded_and_keep_distinct_non_ascii_ids() {
        assert_eq!(semantic_name("device", "enabled"), "device-enabled");
        for id in ["Ääni 🌲", "volume.1", &"large".repeat(100)] {
            let name = semantic_name("device", id);
            assert!(valid_name(&name));
            assert_eq!(name, semantic_name("device", id));
        }
        assert_ne!(semantic_name("device", "volume.1"), semantic_name("device", "volume-1"));
    }

    #[test]
    fn placement_is_bounded_and_round_trips_without_changing_the_native_default() {
        let native = Rect::from_min_size(egui::pos2(30.0, 40.0), Vec2::new(20.0, 60.0));
        let bounds = Rect::from_min_size(egui::Pos2::ZERO, Vec2::splat(100.0));
        let mut p = Placement::default();
        assert_eq!(p.apply(native, bounds), native);
        assert!(p.set("width", "NaNpx").is_err());
        assert!(p.set("left", "2147483648px").is_err());
        assert!(p.set("--suhina-direction", "diagonal").is_err());
        assert!(p.set("width", "4096px").is_ok());
        assert!(p.set("left", "-16384px").is_ok());
        let placed = p.apply(native, bounds);
        assert!(bounds.contains_rect(placed));
        assert_eq!(placed.width(), 100.0);
        let mut warnings = Vec::new();
        assert_eq!(Placement::from_json(&p.to_json(), &mut warnings), p);
        assert!(warnings.is_empty());
    }
}
