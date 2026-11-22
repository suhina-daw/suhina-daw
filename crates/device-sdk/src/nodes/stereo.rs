// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{NodeInfo, PortInfo};

pub const STEREO: NodeInfo = NodeInfo {
    kind: "stereo",
    name: "Stereo",
    inputs: &[
        PortInfo { name: "left", default: None, unit: "signal" },
        PortInfo { name: "right", default: None, unit: "signal" },
    ],
    outputs: &["out", "left", "right"],
    description: "Left channel of left input and right channel of right input become a stereo pair.",
    configuration: "none",
    example: "{\"kind\":\"stereo\"}",
    rust_variant: "Stereo",
};

pub const MONO: NodeInfo = NodeInfo {
    kind: "mono",
    name: "Mono",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Broadcasts (left+right)/2 to both channels.",
    configuration: "none",
    example: "{\"kind\":\"mono\"}",
    rust_variant: "Mono",
};

pub const MID_SIDE_ENCODE: NodeInfo = NodeInfo {
    kind: "mid_side_encode",
    name: "Mid/Side Encode",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Outputs mid=(L+R)/2 in left, side=(L-R)/2 in right.",
    configuration: "none",
    example: "{\"kind\":\"mid_side_encode\"}",
    rust_variant: "MidSideEncode",
};

pub const MID_SIDE_DECODE: NodeInfo = NodeInfo {
    kind: "mid_side_decode",
    name: "Mid/Side Decode",
    inputs: &[PortInfo { name: "in", default: None, unit: "signal" }],
    outputs: &["out", "left", "right"],
    description: "Outputs left=mid+side and right=mid-side.",
    configuration: "none",
    example: "{\"kind\":\"mid_side_decode\"}",
    rust_variant: "MidSideDecode",
};

pub const PAN: NodeInfo = NodeInfo {
    kind: "pan",
    name: "Pan",
    inputs: &[
        PortInfo { name: "in", default: None, unit: "signal" },
        PortInfo { name: "balance", default: Some(0.0), unit: "balance" },
    ],
    outputs: &["out", "left", "right"],
    description: "Balance -1..1: unity at center, attenuates the opposite channel to zero at the extremes.",
    configuration: "none",
    example: "{\"kind\":\"pan\"}",
    rust_variant: "Pan",
};
