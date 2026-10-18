// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUHINA_DEVICES_DIR");
    let folder = if let Some(folder) = std::env::var_os("SUHINA_DEVICES_DIR") {
        std::path::PathBuf::from(folder)
    } else {
        let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("../..");
        let target =
            std::env::var_os("CARGO_TARGET_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| root.join("target"));
        let target = if std::env::var("TARGET")? != std::env::var("HOST")? {
            target.join(std::env::var("TARGET")?)
        } else {
            target
        };
        target.join(std::env::var("PROFILE")?).join("devices")
    };
    println!("cargo:rerun-if-changed={}", folder.display());
    suhina_device_bundle::write_to(&folder)?;
    Ok(())
}
