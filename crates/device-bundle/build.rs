// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let sources = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("../../devices");
    println!("cargo:rerun-if-changed={}", sources.display());
    for directory in std::fs::read_dir(&sources)? {
        let directory = directory?;
        if !directory.path().join("Cargo.toml").is_file() {
            continue;
        }
        let id = directory.file_name().to_string_lossy().into_owned();
        if suhina_device_registry::info(&id).is_none() {
            return Err(format!("devices/{id} must be registered before it can be bundled").into());
        }
    }
    let output = PathBuf::from(std::env::var("OUT_DIR")?);
    std::fs::create_dir_all(output.join("devices"))?;
    let mut generated = String::from("pub static DEVICES: &[StockDevice] = &[\n");
    for info in suhina_device_registry::DEVICES {
        let graph = suhina_device_registry::stock::definition(info.id)?;
        let file = output.join("devices").join(format!("{}.sd", info.id));
        std::fs::write(&file, graph.to_sd()?)?;
        generated.push_str(&format!(
            "StockDevice {{ id: {:?}, name: {:?}, instrument: {}, sd: include_str!({:?}) }},\n",
            info.id,
            info.name,
            graph.role == suhina_device_sdk::DeviceRole::Instrument,
            file
        ));
    }
    generated.push_str("];\n");
    std::fs::write(output.join("bundle.rs"), generated)?;
    Ok(())
}
