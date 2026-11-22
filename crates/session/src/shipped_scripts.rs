// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped surface programs (docs/spec/shipped-scripts.md, #1676): the scripts Suhina
//! carries inside the executable and unpacks into the library's `Scripts/` folder at start.
//! The pack's own rule ([ADR 0036] › 3): each file written only when it is not there, a file
//! the user carries never overwritten, never moved, and a script that could not be written
//! said in the start-up warnings while the next start tries again.

use std::path::Path;

/// One shipped script: its manifest's `id` — the folder it unpacks into under `Scripts/` — and
/// its files as `(name, bytes)`, embedded from the tree's `packs/<id>/` ([ADR 0036] › 3).
/// Beside the script ride its manifest and a README carrying where the script's knowledge
/// comes from and its licence ([ADR 0036] › 1 and 6).
pub struct ShippedScript {
    pub id: &'static str,
    pub files: &'static [(&'static str, &'static [u8])],
}

/// Every shipped script, in the order the unpack walks them. A test keeps this list and the
/// `packs/<id>/` folders the same, so a file added to a pack is never forgotten here.
pub fn scripts() -> &'static [ShippedScript] {
    static SCRIPTS: &[ShippedScript] = &[ShippedScript {
        id: "apc40-mkii",
        files: &[
            ("extension.json", include_bytes!("../../../packs/apc40-mkii/extension.json")),
            ("script.lua", include_bytes!("../../../packs/apc40-mkii/script.lua")),
            ("README.md", include_bytes!("../../../packs/apc40-mkii/README.md")),
        ],
    }];
    SCRIPTS
}

/// Unpacks every shipped script into `scripts_root` — the library's `Scripts/` folder
/// (`Documents/Suhina/Scripts`, [ADR 0036] › 2) — each file only when it is missing: a file
/// the user carries is theirs, never overwritten, never moved. Returns how many files were
/// written and the lines a script that could not be written should say, one per script — the
/// next start tries again. `SUHINA_PACK_DEBUG=1` prints the walk to stderr, like the NAM
/// pack's unpack.
pub fn unpack(scripts_root: &Path) -> (usize, Vec<String>) {
    let debug = std::env::var("SUHINA_PACK_DEBUG").as_deref() == Ok("1");
    let mut written = 0;
    let mut warnings = Vec::new();
    for script in scripts() {
        match unpack_one(scripts_root, script) {
            Ok(count) => {
                if debug {
                    eprintln!("shipped scripts: {} wrote {count} file(s)", script.id);
                }
                written += count;
            }
            Err(e) => {
                if debug {
                    eprintln!("shipped scripts: {} did not unpack: {e}", script.id);
                }
                warnings.push(format!(
                    "the shipped {} script could not be written into {}: {e}; the next start tries again",
                    script.id,
                    scripts_root.join(script.id).display()
                ));
            }
        }
    }
    if debug {
        eprintln!("shipped scripts: {written} file(s) written");
    }
    (written, warnings)
}

/// One script's files into its folder under `scripts_root`, each only when it is missing. The
/// first file that cannot be written stops the script with its error: the folder is one
/// write, and one line about it is enough. Files already there are not touched and not
/// counted.
fn unpack_one(scripts_root: &Path, script: &ShippedScript) -> std::io::Result<usize> {
    let folder = scripts_root.join(script.id);
    let mut written = 0;
    for &(name, bytes) in script.files {
        let target = folder.join(name);
        if target.exists() {
            continue;
        }
        std::fs::create_dir_all(&folder)?;
        if publish(&target, bytes)? {
            written += 1;
        }
    }
    Ok(written)
}

/// Writes `bytes` beside `target` first, then renames the completed file into place — a write
/// that fails (or an app that ends mid-write) leaves no partial file at the target
/// ([`crate::files::stage_and_rename`]). `false` for a file that appeared while this ran:
/// another start's, never replaced.
fn publish(target: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    if target.exists() {
        return Ok(false);
    }
    let staging =
        target.with_file_name(format!("{}.suhina-new", target.file_name().unwrap_or_default().to_string_lossy()));
    crate::files::stage_and_rename(&staging, target, bytes)?;
    Ok(true)
}

#[cfg(test)]
#[path = "shipped_scripts_tests.rs"]
mod tests;
