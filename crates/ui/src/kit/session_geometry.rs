// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme-owned Session density; native render, scroll and drop geometry share it.
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionGeometry {
    pub track_width: Option<u16>,
    pub return_width: Option<u16>,
    pub scene_width: Option<u16>,
    pub slot_height: Option<u16>,
    pub header_height: Option<u16>,
}

impl SessionGeometry {
    pub fn entries(self) -> [(&'static str, Option<u16>); 5] {
        [
            ("track-width", self.track_width),
            ("return-width", self.return_width),
            ("scene-width", self.scene_width),
            ("slot-height", self.slot_height),
            ("header-height", self.header_height),
        ]
    }

    pub fn limits(name: &str) -> Option<(u16, u16)> {
        match name {
            "track-width" | "return-width" | "scene-width" => Some((64, 512)),
            "slot-height" => Some((16, 64)),
            "header-height" => Some((20, 64)),
            _ => None,
        }
    }

    pub fn set(&mut self, name: &str, value: &str) -> Result<(), &'static str> {
        let (min, max) = Self::limits(name).ok_or("unknown Session dimension")?;
        let value = if value == "auto" {
            None
        } else {
            Some(
                value
                    .strip_suffix("px")
                    .and_then(|s| s.parse::<u16>().ok())
                    .filter(|n| (min..=max).contains(n))
                    .ok_or("Session dimension is outside its supported range")?,
            )
        };
        match name {
            "track-width" => self.track_width = value,
            "return-width" => self.return_width = value,
            "scene-width" => self.scene_width = value,
            "slot-height" => self.slot_height = value,
            "header-height" => self.header_height = value,
            _ => return Err("unknown Session dimension"),
        }
        Ok(())
    }

    pub fn to_json(self) -> Value {
        Value::Object(
            self.entries().into_iter().filter_map(|(name, value)| value.map(|v| (name.into(), v.into()))).collect(),
        )
    }

    pub fn from_json(value: &Value, warnings: &mut Vec<String>) -> Self {
        let mut result = Self::default();
        let Some(properties) = value.as_object().filter(|p| p.len() <= 5) else {
            warnings.push("skin.session must be an object of at most five dimensions".into());
            return result;
        };
        for (name, value) in properties {
            if Self::limits(name).is_none() {
                warnings.push(format!("skin.session.{name}: unknown Session dimension"));
                continue;
            }
            if value.is_null() {
                continue;
            }
            if let Some(value) = value.as_u64() {
                if let Err(error) = result.set(name, &format!("{value}px")) {
                    warnings.push(format!("skin.session.{name}: {error}"));
                }
            } else {
                warnings.push(format!("skin.session.{name}: expected an integer"));
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn density_round_trips_and_rejects_unknown_nonfinite_or_unbounded_dimensions() {
        let source =
            json!({"track-width": 80, "return-width": 72, "scene-width": 80, "slot-height": 18, "header-height": 24});
        let mut warnings = Vec::new();
        let mut geometry = SessionGeometry::from_json(&source, &mut warnings);
        assert!(warnings.is_empty());
        assert_eq!(geometry.to_json(), source);
        for (name, value) in [
            ("track-width", "63px"),
            ("slot-height", "0px"),
            ("header-height", "NaNpx"),
            ("scene-width", "999999999999px"),
            ("display", "none"),
        ] {
            let before = geometry;
            assert!(geometry.set(name, value).is_err());
            assert_eq!(geometry, before);
        }
        geometry.set("track-width", "auto").unwrap();
        assert!(geometry.track_width.is_none());
        SessionGeometry::from_json(&json!({"track-width": -1, "slot-height": 0.5, "unknown": 1}), &mut warnings);
        assert_eq!(warnings.len(), 3);
        assert_eq!(SessionGeometry::default().to_json(), Value::Object(serde_json::Map::new()));
    }
}
