// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bridge for 32-bit Windows plug-ins (ADR 0006 › 11): the same plug-in host, built for
//! 32-bit x86 (`suhina-plugin-host-32.exe`), beside the 64-bit one. A plug-in file whose code is
//! 32-bit x86 only is scanned and loaded by it instead of the 64-bit host, over the same
//! protocol and shared memory (both are laid out in fixed-width words, so a 32-bit process
//! reads them as a 64-bit one does). Instances in it are grouped apart from the 64-bit
//! host's, since a group is keyed by its host ([`crate::Launch::group`]).
//!
//! Without the 32-bit host beside the 64-bit one, nothing changes: such a file is listed as
//! built for another processor, as before.

use std::path::{Path, PathBuf};

use crate::arch::Arch;

/// The 32-bit host's file name without the extension.
pub const HOST_STEM: &str = "suhina-plugin-host-32";

/// The 32-bit host that belongs beside `host` (whether it is there or not).
pub fn host_beside(host: &Path) -> PathBuf {
    let name = format!("{HOST_STEM}{}", std::env::consts::EXE_SUFFIX);
    match host.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

/// Whether a program running on `here` bridges a plug-in built for `archs` only: a 64-bit
/// Windows program and a plug-in with no 64-bit code but 32-bit x86 code.
pub fn bridges(archs: &[Arch], here: Option<Arch>, windows: bool) -> bool {
    windows && here == Some(Arch::X86_64) && !archs.contains(&Arch::X86_64) && archs.contains(&Arch::X86)
}

/// The 32-bit host beside `host`, when it is there and the plug-in `file` needs it; `None`
/// when `host` loads the file itself (or nothing here can).
pub fn bridge_for_file(host: &Path, file: &Path) -> Option<PathBuf> {
    let archs = crate::arch::incompatible(file)?;
    if !bridges(&archs, Arch::this_program(), cfg!(windows)) {
        return None;
    }
    let bridge = host_beside(host);
    bridge.is_file().then_some(bridge)
}

/// The host that loads the plug-in file `file`: the 32-bit host beside `host` for a 32-bit
/// plug-in, when it is there, and `host` for every other.
pub fn host_for_file(host: &Path, file: &Path) -> PathBuf {
    bridge_for_file(host, file).unwrap_or_else(|| host.to_path_buf())
}

/// The plug-in file a load spec names: `<format>:<file>[#<id>]`; `None` for one that names no
/// file (`test:…`).
pub fn spec_file(spec: &str) -> Option<PathBuf> {
    let (format, rest) = spec.split_once(':')?;
    if !matches!(format, "clap" | "vst3" | "vst2") {
        return None;
    }
    // The file may hold a `#` itself: the whole rest first, then without the id.
    let whole = Path::new(rest);
    if whole.exists() {
        return Some(whole.to_path_buf());
    }
    rest.rsplit_once('#').map(|(file, _)| PathBuf::from(file))
}

/// The host that loads the plug-in `spec` names ([`host_for_file`]); `host` for a spec that
/// names no file.
pub fn host_for_spec(host: &Path, spec: &str) -> PathBuf {
    match spec_file(spec) {
        Some(file) => host_for_file(host, &file),
        None => host.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_32_bit_x86_plugin_on_64_bit_windows_is_bridged() {
        let x64 = Some(Arch::X86_64);
        assert!(bridges(&[Arch::X86], x64, true));
        // Not on another system, not from another processor, not with 64-bit code in it.
        assert!(!bridges(&[Arch::X86], x64, false));
        assert!(!bridges(&[Arch::X86], Some(Arch::Arm64), true));
        assert!(!bridges(&[Arch::X86, Arch::X86_64], x64, true));
        assert!(!bridges(&[Arch::Arm], x64, true));
        assert!(!bridges(&[Arch::X86], None, true));
    }

    #[test]
    fn a_spec_names_its_file() {
        assert_eq!(spec_file(r"vst2:C:\VST\Old Synth.dll#1234"), Some(PathBuf::from(r"C:\VST\Old Synth.dll")));
        assert_eq!(spec_file("clap:/p/Thing.clap#com.maker.thing"), Some(PathBuf::from("/p/Thing.clap")));
        assert_eq!(spec_file("vst3:/p/A#B.vst3#ABCD"), Some(PathBuf::from("/p/A#B.vst3")));
        assert_eq!(spec_file("test:vst2"), None);
        assert_eq!(spec_file("test:crash-after:3"), None);
        assert_eq!(spec_file("nonsense"), None);
    }

    #[test]
    fn the_bridge_sits_beside_the_host() {
        let host = Path::new("dir").join(format!("suhina-plugin-host{}", std::env::consts::EXE_SUFFIX));
        let bridge = host_beside(&host);
        assert_eq!(bridge.parent(), Some(Path::new("dir")));
        assert!(bridge.to_string_lossy().contains(HOST_STEM));
        assert!(host_beside(Path::new("suhina-plugin-host")).to_string_lossy().starts_with(HOST_STEM));
    }

    #[test]
    fn a_32_bit_dll_goes_to_the_bridge_when_it_is_there() {
        let dir = std::env::temp_dir().join(format!("suhina-bridge-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut pe = vec![0u8; 0x100];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x014C_u16.to_le_bytes());
        let old = dir.join("Old.dll");
        std::fs::write(&old, &pe).unwrap();
        let host = dir.join(format!("suhina-plugin-host{}", std::env::consts::EXE_SUFFIX));
        let spec = format!("vst2:{}#1", old.display());
        // No bridge yet: the host itself.
        assert_eq!(host_for_spec(&host, &spec), host);
        std::fs::write(host_beside(&host), b"").unwrap();
        let expected =
            if cfg!(windows) && Arch::this_program() == Some(Arch::X86_64) { host_beside(&host) } else { host.clone() };
        assert_eq!(host_for_spec(&host, &spec), expected);
        // A 64-bit file stays with the host.
        pe[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
        std::fs::write(&old, &pe).unwrap();
        assert_eq!(host_for_spec(&host, &spec), host);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
