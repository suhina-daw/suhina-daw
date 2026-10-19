// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The version information Windows shows for suhina.exe: the product name, what the program
//! is (Task Manager's name for it) and the version, which code signing checks too (#180); and
//! the stock `.sd` definitions written beside the app (#600).

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=SUHINA_DEVICES_DIR");
    let folder = if let Some(folder) = std::env::var_os("SUHINA_DEVICES_DIR") {
        std::path::PathBuf::from(folder)
    } else {
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("devices")
    };
    suhina_device_bundle::write_to(&folder)?;
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set("ProductName", "Suhina")
            .set("FileDescription", "Suhina")
            .set("CompanyName", "The Suhina contributors")
            .set("LegalCopyright", "Copyright (C) 2026 The Suhina contributors, GPL-3.0-or-later")
            .compile()?;
    }
    Ok(())
}
