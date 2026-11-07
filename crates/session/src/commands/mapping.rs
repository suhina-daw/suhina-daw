// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `mapping.*` commands: the project's controller mappings (`docs/spec/midi-mapping.md`).
//! What they run is in [`crate::mapping`].

use super::*;

const SOURCE: &str = "key:<name> (key:Q, key:1, key:Space) or note:<channel 1-16>:<note 0-127>";
const TARGET: &str = "{\"command\": \"clip.launch\", \"track\": id or name, \"scene\": n}, {\"command\": \"scene.launch\", \"scene\": n} or {\"command\": \"track.stop\", \"track\": id or name}; a scene is its number, from 0, or {\"id\": n} with its scene_id";

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "mapping.list",
        label: "List the project's controller mappings: each one's index, source, target command with its track and scene, and whether its target is gone",
        params: &[],
        kind: CommandKind::READ,
        run: crate::mapping::list_mappings,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "mapping.add",
        label: "Map a computer key or a MIDI note to a command: a slot's clip.launch (its release runs clip.release for a gate or repeat clip), a scene.launch or a track.stop; a source already mapped moves to the new target",
        params: &[param("source", ParamType::Text, SOURCE), param("target", ParamType::MappingTarget, TARGET)],
        kind: CommandKind::EDIT,
        run: crate::mapping::add_mapping,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "mapping.remove",
        label: "Remove controller mappings: one by its index in mapping.list or by its source, or every mapping of a target",
        params: &[
            optional("index", ParamType::Integer, "the mapping's index in mapping.list, from 0"),
            optional("source", ParamType::Text, "key:<name> or note:<channel>:<note>"),
            optional("target", ParamType::MappingTarget, TARGET),
        ],
        kind: CommandKind::EDIT,
        run: crate::mapping::remove_mapping,
        policy: Policy::DEFAULT,
    },
];
