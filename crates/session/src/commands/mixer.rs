// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `main.*`, `mixer.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "mixer.setMainVolume",
        label: "Set the main output's volume, which every track, group and return plays through",
        params: &[
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
            optional(
                "continue",
                ParamType::Bool,
                "true adds the change to the undo step of the previous call of this command, when no other \
                 edit came between (one drag, one step)",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_main_volume,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "mixer.levels",
        label: "Read the level meters of the playing engine: the main output's and each track's peak and RMS level per side in dB (null for silence), and whether it clipped",
        params: &[],
        kind: CommandKind::Transport,
        run: meters::levels,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "mixer.resetClip",
        label: "Turn off clip indicators: a track's, the main output's, or every one when neither is given",
        params: &[
            optional("track", ParamType::Track, "track id or name, from project.tracks"),
            optional("main", ParamType::Bool, "true turns off the main output's"),
        ],
        kind: CommandKind::Transport,
        run: meters::reset_clip,
        policy: Policy::DEFAULT,
    },
];

fn set_main_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > MAX_VOLUME_DB {
        return Err(format!("db must be at most +{MAX_VOLUME_DB}, got {db}").into());
    }
    let Some(main) = session.project.channels.iter_mut().find(|c| c.role == ChannelRole::Master) else {
        return Err("the project has no main channel".into());
    };
    main.volume = gain_of_db(db);
    Ok(json!({ "volume_db": to_db(main.volume) }))
}
