// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The version information Windows shows for suhina-cli.exe: the product name, what the program
//! is (Task Manager's name for it) and the version, which code signing checks too (#180).

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    winresource::WindowsResource::new()
        .set("ProductName", "Suhina")
        .set("FileDescription", "Suhina command line")
        .set("CompanyName", "The Suhina contributors")
        .set("LegalCopyright", "Copyright (C) 2026 The Suhina contributors, GPL-3.0-or-later")
        .compile()
}
