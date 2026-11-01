// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests load standalone example crates, each with its own local helpers.
#![allow(clippy::duplicate_mod)]
#[path = "../../examples/compressor.rs"]
mod compressor;
#[path = "../../examples/delay.rs"]
mod delay;
#[path = "../../examples/distortion.rs"]
mod distortion;
#[path = "../../examples/filter.rs"]
mod filter;
#[path = "../../examples/lfo_pan.rs"]
mod lfo_pan;
#[path = "../../examples/stereo_utility.rs"]
mod stereo_utility;
pub fn build(name: &str) -> Result<suhina_device_sdk::DeviceGraph, Box<dyn std::error::Error>> {
    match name {
        "stereo-utility" => stereo_utility::build(),
        "lfo-pan" => lfo_pan::build(),
        "filter" => filter::build(),
        "distortion" => distortion::build(),
        "delay" => delay::build(),
        "compressor" => compressor::build(),
        _ => Err(format!("unknown example {name:?}").into()),
    }
}
