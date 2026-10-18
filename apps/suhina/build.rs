// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUHINA_DEVICES_DIR");
    let folder = if let Some(folder) = std::env::var_os("SUHINA_DEVICES_DIR") {
        std::path::PathBuf::from(folder)
    } else {
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("devices")
    };
    suhina_device_bundle::write_to(&folder)?;
    Ok(())
}
