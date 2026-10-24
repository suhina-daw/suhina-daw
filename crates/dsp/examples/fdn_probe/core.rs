// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::{Value, json};
use std::{error::Error, path::Path};
use suhina_dsp::fdn::{Fdn, Settings, SettingsError};
use suhina_dsp::fdn_diffuse::{DiffuseSettings, DiffusedFdn};

pub struct Options {
    pub input: Option<std::path::PathBuf>,
    pub profile: String,
    pub flat_decay: bool,
    pub diffused: bool,
    pub render_only: bool,
    pub impulse_only: bool,
    local: Value,
}

impl Options {
    pub fn read() -> Result<Self, Box<dyn Error>> {
        let mut result = Self {
            input: None,
            profile: "baseline".into(),
            flat_decay: false,
            diffused: false,
            render_only: false,
            impulse_only: false,
            local: Value::Null,
        };
        let mut args = std::env::args_os().skip(2);
        while let Some(arg) = args.next() {
            if arg == "--input" {
                result.input = Some(args.next().ok_or("--input needs a WAV path")?.into());
            } else if arg == "--profile" {
                result.profile = args
                    .next()
                    .ok_or("--profile needs short, baseline or long")?
                    .into_string()
                    .map_err(|_| "invalid profile")?;
                if !["short", "baseline", "long"].contains(&result.profile.as_str()) {
                    return Err("invalid profile".into());
                }
            } else if arg == "--flat-decay" {
                result.flat_decay = true;
            } else if arg == "--diffused" {
                result.diffused = true;
            } else if arg == "--render-only" {
                result.render_only = true;
            } else if arg == "--impulse-only" {
                result.impulse_only = true;
            } else if arg == "--settings" {
                let path = args.next().ok_or("--settings needs a JSON path")?;
                if std::fs::metadata(Path::new(&path))?.len() > 65_536 {
                    return Err("settings JSON exceeds 64 KiB".into());
                }
                result.local = serde_json::from_slice(&std::fs::read(path)?)?;
                if !result.local.is_object() {
                    return Err("settings JSON must be an object".into());
                }
            } else {
                return Err("unknown option; use --flat-decay, --diffused, --render-only or --settings <json>".into());
            }
        }
        Ok(result)
    }

    pub fn settings(&self, name: &str, mut core: Settings) -> Result<DiffuseSettings, Box<dyn Error>> {
        let mut s = DiffuseSettings { core, ..DiffuseSettings::default() };
        if let Some(value) = self.local.get(name) {
            let object = value.as_object().ok_or("per-render settings must be an object")?;
            for (key, value) in object {
                let number = value.as_f64().ok_or("setting must be a finite number")?;
                match key.as_str() {
                    "decay_seconds" => core.decay_seconds = number,
                    "room_scale" => core.room_scale = number,
                    "late_scale" => s.late_scale = number,
                    "high_decay_ratio" => core.high_decay_ratio = number,
                    "feedback_rotation" => core.feedback_rotation = number,
                    "input_rotation" => core.input_rotation = number,
                    "output_rotation" => core.output_rotation = number,
                    "loop_diffusion" => core.loop_diffusion = number as f32,
                    "predelay_ms" => s.predelay_ms = number,
                    "diffusion" => s.diffusion = number as f32,
                    "early_level" => s.early_level = number as f32,
                    "early_onset_gain" => s.early_onset_gain = number as f32,
                    "early_tilt_db" => s.early_tilt_db = number,
                    "early_diffusion" => s.early_diffusion = number as f32,
                    "width" => s.width = number as f32,
                    "low_cut_hz" => s.low_cut_hz = number,
                    "high_cut_hz" => s.high_cut_hz = number,
                    _ => return Err(format!("unknown setting {key}").into()),
                }
            }
        }
        s.core = core;
        Ok(s)
    }
}

pub enum Core {
    Plain(Box<Fdn>),
    Diffused(Box<DiffusedFdn>),
}

impl Core {
    pub fn new(s: DiffuseSettings, diffused: bool) -> Result<Self, SettingsError> {
        if diffused {
            Ok(Self::Diffused(Box::new(DiffusedFdn::new(s)?)))
        } else {
            Ok(Self::Plain(Box::new(Fdn::new(s.core)?)))
        }
    }
    pub fn process(&mut self, samples: &mut [f32]) {
        match self {
            Self::Plain(core) => core.process(samples),
            Self::Diffused(core) => core.process(samples),
        }
    }
    pub fn process_frame(&mut self, frame: [f32; 2]) -> [f32; 2] {
        match self {
            Self::Plain(core) => core.process_frame(frame),
            Self::Diffused(core) => core.process_frame(frame),
        }
    }
    pub fn history_bytes(&self) -> usize {
        match self {
            Self::Plain(core) => core.history_bytes(),
            Self::Diffused(core) => core.history_bytes(),
        }
    }
}

pub fn describe(s: DiffuseSettings) -> Value {
    json!({"feedback_rotation": s.core.feedback_rotation, "input_rotation": s.core.input_rotation, "output_rotation": s.core.output_rotation, "loop_diffusion": s.core.loop_diffusion, "late_scale": s.late_scale, "predelay_ms": s.predelay_ms, "diffusion": s.diffusion, "early_level": s.early_level,
        "early_onset_gain": s.early_onset_gain, "early_tilt_db": s.early_tilt_db, "early_diffusion": s.early_diffusion,
        "width": s.width, "low_cut_hz": s.low_cut_hz, "high_cut_hz": s.high_cut_hz})
}
