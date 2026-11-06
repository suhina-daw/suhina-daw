// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded, theme-owned surface gradients. Colours are overlays, never project edits.

use egui::{Color32, Rect, vec2};
use serde_json::{Value, json};

use super::{motion, theme};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub position: u8,
    pub color: Color32,
    pub transparent_rgb: Option<[u8; 3]>,
}

impl Stop {
    pub fn rgba(&self) -> [u8; 4] {
        let mut rgba = self.color.to_srgba_unmultiplied();
        if rgba[3] == 0
            && let Some(rgb) = self.transparent_rgb
        {
            rgba[..3].copy_from_slice(&rgb);
        }
        rgba
    }
    pub fn set_rgba(&mut self, [r, g, b, a]: [u8; 4]) {
        self.transparent_rgb = (a == 0).then_some([r, g, b]);
        self.color = Color32::from_rgba_unmultiplied(r, g, b, a);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gradient {
    /// Clockwise: 0 is left to right; 90 is top to bottom.
    pub angle: u16,
    pub stops: Vec<Stop>,
}

impl Default for Gradient {
    fn default() -> Self {
        Self {
            angle: 90,
            stops: vec![
                Stop { transparent_rgb: None, position: 0, color: Color32::from_white_alpha(48) },
                Stop { transparent_rgb: None, position: 100, color: Color32::from_black_alpha(40) },
            ],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gradients {
    pub panels: Option<Gradient>,
    pub tracks: Option<Gradient>,
    pub clips: Option<Gradient>,
}

impl Gradients {
    pub fn from_json(value: &Value, warnings: &mut Vec<String>) -> Self {
        let mut result = Self::default();
        let Some(root) = value.as_object() else {
            warnings.push("gradients must be an object of panels, tracks and clips".into());
            return result;
        };
        for (key, slot) in
            [("panels", &mut result.panels), ("tracks", &mut result.tracks), ("clips", &mut result.clips)]
        {
            let Some(value) = root.get(key) else { continue };
            *slot = Gradient::from_json(value);
            if slot.is_none() {
                warnings.push(format!("gradients.{key}: use angle 0–360 and 2–8 stops with position 0–100 and colour"));
            }
        }
        for key in root.keys().filter(|key| !matches!(key.as_str(), "panels" | "tracks" | "clips")) {
            warnings.push(format!("gradients.{key} is not a surface"));
        }
        result
    }

    pub fn to_json(&self) -> Value {
        let mut root = serde_json::Map::new();
        for (name, value) in [("panels", &self.panels), ("tracks", &self.tracks), ("clips", &self.clips)] {
            if let Some(value) = value {
                root.insert(
                    name.into(),
                    json!({"angle":value.angle,"stops":value.stops.iter().map(|stop|
                    json!({"position":stop.position,"color":theme::hex_rgba(stop.rgba())})
                ).collect::<Vec<_>>()}),
                );
            }
        }
        Value::Object(root)
    }
}

impl Gradient {
    fn from_json(value: &Value) -> Option<Self> {
        let angle = value.get("angle")?.as_u64().filter(|angle| *angle <= 360)? as u16;
        let values = value.get("stops")?.as_array().filter(|stops| (2..=8).contains(&stops.len()))?;
        let mut stops = Vec::with_capacity(values.len());
        for value in values {
            let rgba = theme::parse_rgba(value.get("color")?.as_str()?)?;
            stops.push(Stop {
                transparent_rgb: (rgba[3] == 0).then_some([rgba[0], rgba[1], rgba[2]]),
                position: value.get("position")?.as_u64().filter(|position| *position <= 100)? as u8,
                color: theme::parse_hex(value.get("color")?.as_str()?)?,
            });
        }
        stops.sort_by_key(|stop| stop.position);
        Some(Self { angle, stops })
    }

    /// Split at stop planes, so intermediate stops stay exact even at diagonal angles.
    pub fn mesh(&self, rect: Rect) -> egui::Mesh {
        let mut mesh = egui::Mesh::default();
        if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 || !(2..=8).contains(&self.stops.len()) {
            return mesh;
        }
        let mut stops = self.stops.clone();
        stops.sort_by_key(|stop| stop.position);
        let direction = vec2((self.angle as f32).to_radians().cos(), (self.angle as f32).to_radians().sin());
        let radius = (direction.x.abs() * rect.width() + direction.y.abs() * rect.height()) * 0.5;
        let coordinate =
            |point: egui::Pos2| ((point - rect.center()).dot(direction) / (2.0 * radius) + 0.5).clamp(0.0, 1.0);
        let first = stops[0].color;
        let last = stops[stops.len() - 1].color;
        stops.insert(0, Stop { transparent_rgb: None, position: 0, color: first });
        stops.push(Stop { transparent_rgb: None, position: 100, color: last });
        for pair in stops.windows(2) {
            let low = f32::from(pair[0].position.min(100)) / 100.0;
            let high = f32::from(pair[1].position.min(100)) / 100.0;
            if high <= low {
                continue;
            }
            let mut polygon = vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
            for (bound, keep_above) in [(low, true), (high, false)] {
                let input = std::mem::take(&mut polygon);
                for (from, to) in input.iter().zip(input.iter().cycle().skip(1)).take(input.len()) {
                    let a = coordinate(*from);
                    let b = coordinate(*to);
                    let inside_a = if keep_above { a >= bound } else { a <= bound };
                    let inside_b = if keep_above { b >= bound } else { b <= bound };
                    if inside_a {
                        polygon.push(*from);
                    }
                    if inside_a != inside_b && (b - a).abs() > f32::EPSILON {
                        polygon.push(*from + (*to - *from) * ((bound - a) / (b - a)));
                    }
                }
            }
            let start = mesh.vertices.len() as u32;
            for point in &polygon {
                mesh.colored_vertex(
                    *point,
                    motion::mix(pair[0].color, pair[1].color, (coordinate(*point) - low) / (high - low)),
                );
            }
            for index in 1..polygon.len().saturating_sub(1) {
                mesh.add_triangle(start, start + index as u32, start + index as u32 + 1);
            }
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn transparent_stop_keeps_its_hue_on_disk_and_when_opacity_returns() {
        let mut warnings = Vec::new();
        let mut gradients = Gradients::from_json(
            &json!({"panels":{"angle":90,"stops":[{"position":0,"color":"#FA802000"},{"position":100,"color":"#FFFFFF"}]}}),
            &mut warnings,
        );
        assert!(warnings.is_empty());
        assert_eq!(gradients.to_json()["panels"]["stops"][0]["color"], "#FA802000");
        let stop = &mut gradients.panels.as_mut().unwrap().stops[0];
        let mut rgba = stop.rgba();
        rgba[3] = 255;
        stop.set_rgba(rgba);
        assert_eq!(stop.color, Color32::from_rgb(250, 128, 32));
    }

    #[test]
    fn diagonal_stops_keep_exact_colours_and_bounded_geometry() {
        let gradient = Gradient {
            angle: 37,
            stops: vec![
                Stop { transparent_rgb: None, position: 0, color: Color32::RED },
                Stop { transparent_rgb: None, position: 43, color: Color32::from_white_alpha(100) },
                Stop { transparent_rgb: None, position: 100, color: Color32::BLUE },
            ],
        };
        let rect = Rect::from_min_max(pos2(10.0, 20.0), pos2(310.0, 100.0));
        let mesh = gradient.mesh(rect);
        assert!(mesh.vertices.iter().any(|vertex| vertex.color == gradient.stops[1].color));
        assert!(mesh.vertices.iter().all(|vertex| rect.expand(0.001).contains(vertex.pos)));
        assert!(mesh.vertices.len() <= 36 && !mesh.indices.is_empty());
        let gradients = Gradients { clips: Some(gradient), ..Default::default() };
        let mut warnings = Vec::new();
        assert_eq!(Gradients::from_json(&gradients.to_json(), &mut warnings), gradients);
        assert!(warnings.is_empty());
    }

    #[test]
    fn invalid_files_and_degenerate_geometry_are_rejected() {
        for value in [
            json!({"angle":361,"stops":[]}),
            json!({"angle":90,"stops":[{"position":101,"color":"#fff"}]}),
            json!({"angle":"bad"}),
        ] {
            let mut warnings = Vec::new();
            assert_eq!(Gradients::from_json(&json!({"clips":value}), &mut warnings), Gradients::default());
            assert_eq!(warnings.len(), 1);
        }
        assert!(Gradient::default().mesh(Rect::NOTHING).is_empty());
    }
}
