// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which processors a plug-in file's code is built for, read from the executable's header
//! without loading it: Mach-O (thin and universal) on macOS, PE on Windows, ELF on Linux. A
//! plug-in built only for another processor (an Intel-only plug-in on an Apple silicon Mac)
//! can never load in Suhina's plug-in host, which runs on the processor Suhina was built for,
//! so a scan settles it at once ([`crate::scan::Scanned::Incompatible`]) instead of loading it
//! and reading the loader's error.

use std::io::Read;
use std::path::{Path, PathBuf};

/// Bytes of an executable read for its header.
const HEADER: u64 = 4096;
/// Most entries a universal Mach-O file may list; a file whose count is above it is not one
/// (a Java class file starts with the same four bytes, then its version).
const MAX_FAT: u32 = 16;

/// A processor a plug-in's code can be built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86,
    X86_64,
    Arm,
    Arm64,
}

impl Arch {
    /// The name it is shown as.
    pub fn name(self) -> &'static str {
        match self {
            Self::X86 => "x86",
            Self::X86_64 => "x86_64",
            Self::Arm => "arm",
            Self::Arm64 => "arm64",
        }
    }

    /// The processor this program (and the plug-in host built with it) runs on; `None` for one
    /// this module does not know.
    pub fn this_program() -> Option<Self> {
        match std::env::consts::ARCH {
            "x86" => Some(Self::X86),
            "x86_64" => Some(Self::X86_64),
            "arm" => Some(Self::Arm),
            "aarch64" => Some(Self::Arm64),
            _ => None,
        }
    }

    fn from_macho(cputype: u32) -> Option<Self> {
        match cputype {
            7 => Some(Self::X86),
            0x0100_0007 => Some(Self::X86_64),
            12 => Some(Self::Arm),
            0x0100_000C => Some(Self::Arm64),
            _ => None,
        }
    }
}

fn u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u16_be(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// The processors the executable starting with `header` is built for; `None` when the header
/// is not one this module reads (the loader then says what it is).
pub fn read(header: &[u8]) -> Option<Vec<Arch>> {
    let magic = u32_be(header, 0)?;
    match magic {
        // Universal Mach-O: a count, then one entry per processor (20 bytes, or 32 in the
        // 64-bit form), each starting with its CPU type. All big-endian.
        0xCAFE_BABE | 0xCAFE_BABF => {
            let count = u32_be(header, 4)?;
            if count == 0 || count > MAX_FAT {
                return None;
            }
            let size = if magic == 0xCAFE_BABE { 20 } else { 32 };
            let archs = (0..count as usize)
                .filter_map(|i| u32_be(header, 8 + i * size))
                .filter_map(Arch::from_macho)
                .collect::<Vec<_>>();
            (!archs.is_empty()).then_some(archs)
        }
        // Thin Mach-O, written by a little-endian machine (every one Suhina runs on) or not.
        0xCEFA_EDFE | 0xCFFA_EDFE => Arch::from_macho(u32_le(header, 4)?).map(|a| vec![a]),
        0xFEED_FACE | 0xFEED_FACF => Arch::from_macho(u32_be(header, 4)?).map(|a| vec![a]),
        // ELF: the machine at 18, in the file's own byte order (byte 5: 1 little, 2 big).
        0x7F45_4C46 => {
            let machine = if header.get(5) == Some(&2) { u16_be(header, 18)? } else { u16_le(header, 18)? };
            match machine {
                3 => Some(vec![Arch::X86]),
                62 => Some(vec![Arch::X86_64]),
                40 => Some(vec![Arch::Arm]),
                183 => Some(vec![Arch::Arm64]),
                _ => None,
            }
        }
        // PE: `MZ`, the offset of the PE header at 0x3C, `PE\0\0` there, then the machine.
        _ if header.starts_with(b"MZ") => {
            let pe = usize::try_from(u32_le(header, 0x3C)?).ok()?;
            if header.get(pe..pe.checked_add(4)?)? != b"PE\0\0" {
                return None;
            }
            match u16_le(header, pe.checked_add(4)?)? {
                0x014C => Some(vec![Arch::X86]),
                0x8664 => Some(vec![Arch::X86_64]),
                0x01C0 | 0x01C4 => Some(vec![Arch::Arm]),
                0xAA64 => Some(vec![Arch::Arm64]),
                // Arm64EC code runs in x64 processes on Arm Windows.
                0xA641 => Some(vec![Arch::X86_64, Arch::Arm64]),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The executable that holds a plug-in's code: the file itself, or in a bundle (a folder) the
/// one the plug-in host loads: `Contents/MacOS/<name>` (or the folder's first file) on macOS,
/// `Contents/x86_64-win/<name>.vst3` on Windows, `Contents/x86_64-linux/<name>.so` on Linux.
pub fn executable(plugin: &Path) -> Option<PathBuf> {
    if plugin.is_file() {
        return Some(plugin.to_path_buf());
    }
    let contents = plugin.join("Contents");
    let stem = plugin.file_stem()?;
    if cfg!(target_os = "macos") {
        let macos = contents.join("MacOS");
        let named = macos.join(stem);
        if named.is_file() {
            return Some(named);
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&macos)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
            .collect();
        files.sort();
        return files.into_iter().next();
    }
    let inner = if cfg!(windows) {
        contents.join("x86_64-win").join(plugin.file_name()?)
    } else {
        contents.join("x86_64-linux").join(stem).with_extension("so")
    };
    inner.is_file().then_some(inner)
}

/// What the plug-in at `plugin` is built for, when that rules out this program's processor:
/// its processors, or `None` when it can load here or this cannot tell (the loader then tries).
pub fn incompatible(plugin: &Path) -> Option<Vec<Arch>> {
    let here = Arch::this_program()?;
    let file = std::fs::File::open(executable(plugin)?).ok()?;
    let mut header = Vec::new();
    file.take(HEADER).read_to_end(&mut header).ok()?;
    let archs = read(&header)?;
    (!archs.contains(&here)).then_some(archs)
}

/// Why a plug-in built for `archs` does not load here, in words.
pub fn reason(archs: &[Arch]) -> String {
    let built: Vec<&str> = archs.iter().map(|a| a.name()).collect();
    let here = Arch::this_program().map_or("another processor", Arch::name);
    format!("built for {} only; Suhina runs on {here}", built.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fat(entries: &[u32]) -> Vec<u8> {
        let mut bytes = 0xCAFE_BABE_u32.to_be_bytes().to_vec();
        bytes.extend((entries.len() as u32).to_be_bytes());
        for cputype in entries {
            bytes.extend(cputype.to_be_bytes());
            bytes.extend([0; 16]);
        }
        bytes
    }

    #[test]
    fn mach_o_headers_name_their_processors() {
        // Intel-only universal files, as the plug-ins that do not load on Apple silicon are.
        assert_eq!(read(&fat(&[7, 0x0100_0007])), Some(vec![Arch::X86, Arch::X86_64]));
        assert_eq!(read(&fat(&[0x0100_0007, 0x0100_000C])), Some(vec![Arch::X86_64, Arch::Arm64]));
        // A thin one, written little-endian.
        let mut thin = 0xFEED_FACF_u32.to_le_bytes().to_vec();
        thin.extend(0x0100_0007_u32.to_le_bytes());
        assert_eq!(read(&thin), Some(vec![Arch::X86_64]));
        // A Java class file shares the universal magic; its version is no count.
        let mut class = 0xCAFE_BABE_u32.to_be_bytes().to_vec();
        class.extend(0x0000_0034_u32.to_be_bytes());
        assert_eq!(read(&class), None);
    }

    #[test]
    fn pe_and_elf_headers_name_their_processors() {
        let mut pe = vec![0u8; 0x100];
        pe[0..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664_u16.to_le_bytes());
        assert_eq!(read(&pe), Some(vec![Arch::X86_64]));
        pe[0x84..0x86].copy_from_slice(&0xAA64_u16.to_le_bytes());
        assert_eq!(read(&pe), Some(vec![Arch::Arm64]));
        // A PE offset past the header read, or no PE signature: not known.
        pe[0x3C..0x40].copy_from_slice(&0xFFFF_FFF0_u32.to_le_bytes());
        assert_eq!(read(&pe), None);

        let mut elf = vec![0u8; 64];
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[5] = 1;
        elf[18..20].copy_from_slice(&183_u16.to_le_bytes());
        assert_eq!(read(&elf), Some(vec![Arch::Arm64]));
        elf[18..20].copy_from_slice(&62_u16.to_le_bytes());
        assert_eq!(read(&elf), Some(vec![Arch::X86_64]));
    }

    #[test]
    fn what_is_not_an_executable_says_nothing() {
        let empty_fat: &[u8] = &[0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 0];
        for junk in [&b""[..], &b"MZ"[..], &b"not a plug-in"[..], empty_fat] {
            assert_eq!(read(junk), None, "{junk:?}");
        }
        assert_eq!(incompatible(Path::new("/no/such/plugin.vst3")), None);
    }

    #[test]
    fn a_file_for_another_processor_is_incompatible_here() {
        // Only the two processors Suhina is built for are checked here.
        let Some(here) = Arch::this_program().filter(|a| matches!(a, Arch::X86_64 | Arch::Arm64)) else { return };
        let other = if here == Arch::Arm64 { 0x0100_0007 } else { 0x0100_000C };
        let dir = std::env::temp_dir().join(format!("suhina-arch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let foreign = dir.join("Foreign.clap");
        std::fs::write(&foreign, fat(&[other])).unwrap();
        let archs = incompatible(&foreign).unwrap();
        assert!(!archs.contains(&here));
        assert!(reason(&archs).contains(here.name()));
        let native = dir.join("Native.clap");
        let cputype = if here == Arch::Arm64 { 0x0100_000C } else { 0x0100_0007 };
        std::fs::write(&native, fat(&[other, cputype])).unwrap();
        assert_eq!(incompatible(&native), None, "a universal file with this processor loads");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
