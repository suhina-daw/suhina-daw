// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the vendored LAME 3.100 encoder (`third_party/lame/`) into a static library.
//! The file list, the defines and the per-architecture intrinsics follow
//! `third_party/lame/VENDORED.md`; `fft.c` there is Suhina's own replacement, not LAME's.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let lame = manifest.join("../../third_party/lame");
    let libmp3lame = lame.join("libmp3lame");

    let sources = [
        "bitstream.c",
        "encoder.c",
        "fft.c",
        "gain_analysis.c",
        "id3tag.c",
        "lame.c",
        "newmdct.c",
        "presets.c",
        "psymodel.c",
        "quantize.c",
        "quantize_pvt.c",
        "reservoir.c",
        "set_get.c",
        "tables.c",
        "takehiro.c",
        "util.c",
        "vbrquantize.c",
        "VbrTag.c",
        "version.c",
    ];

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let x86_family = arch == "x86_64" || arch == "x86";

    let mut build = cc::Build::new();
    build
        .include(manifest.join("lame-config"))
        .include(lame.join("include"))
        .include(&libmp3lame)
        .define("HAVE_CONFIG_H", None)
        .define("FLOAT8", Some("float"))
        .define("REAL_IS_FLOAT", Some("1"))
        .define("BS_FORMAT", Some("BINARY"))
        .define("TAKEHIRO_IEEE754_HACK", None)
        // Third-party C: the upstream files compile with their own warnings, which are not
        // ours to fix; the crate's own sources carry none.
        .warnings(false);
    for source in sources {
        build.file(libmp3lame.join(source));
    }
    // The SSE intrinsics file only compiles where the compiler has xmmintrin.h; elsewhere
    // (aarch64 macOS, ARM Linux) LAME uses its C path instead.
    if x86_family {
        build.define("HAVE_XMMINTRIN_H", None);
        build.file(libmp3lame.join("vector/xmm_quantize_sub.c"));
    }
    build.compile("mp3lame");

    println!("cargo:rerun-if-changed={}", libmp3lame.display());
    println!("cargo:rerun-if-changed={}", manifest.join("lame-config").display());
}
