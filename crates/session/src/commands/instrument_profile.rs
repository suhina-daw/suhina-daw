// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "instrument.adoptModalProfile",
    label: "Adopt a validated modal profile, reset its model controls and enable its full modes in one undo step",
    params: &[
        DEVICE_TRACK,
        param("device", ParamType::DevicePath, "Instrument index or full device path"),
        param(
            "profile",
            ParamType::ModalProfile,
            "referenceKey 0..127, referenceHz 8..20000, modes: 1..16 increasing {ratio, magnitude, decaySeconds}",
        ),
        optional("source", ParamType::Text, "Modal (default), or Hybrid to retain the sample layer"),
    ],
    kind: CommandKind::EDIT,
    run: crate::instrument_profile::adopt,
    policy: Policy::DEFAULT,
}];
