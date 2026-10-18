// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let profile = out.ancestors().nth(3).ok_or("unexpected Cargo output directory")?;
    println!("cargo:rerun-if-changed={}", profile.join("devices").display());
    suhina_device_bundle::write_to(&profile.join("devices"))?;
    Ok(())
}
