// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native-device plug-in exports. The sandbox executable remains a separate target.
//! Build with `--features device-exports --lib` to enable the factories (ADR 0040).

#[cfg(feature = "device-exports")]
pub mod exports;
