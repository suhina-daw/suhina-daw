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
            GESTURE,
        ],
        kind: CommandKind::EDIT,
        run: set_main_volume,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "mixer.levels",
        label: "Read the level meters of the playing engine: the main output's and each track's peak and RMS level per side in dB (null for silence), and whether it clipped",
        params: &[],
        kind: CommandKind::TRANSPORT,
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
        kind: CommandKind::TRANSPORT,
        run: meters::reset_clip,
        policy: Policy::DEFAULT,
    },
];

fn set_main_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = Args::new(params).f64_or_neg_infinity("db")?;
    if db > MAX_VOLUME_DB {
        return Err(format!("db must be at most +{MAX_VOLUME_DB}, got {db}").into());
    }
    let Some(main) = session.project.channels.iter_mut().find(|c| c.role == ChannelRole::Master) else {
        return Err("the project has no main channel".into());
    };
    main.volume = gain_of_db(db);
    let volume = main.volume;
    // A fader move, nothing else: a playing engine takes it as a message where it can (#763 › 4).
    session.sync.mixer_edits = vec![crate::ParamEdit {
        track: suhina_model::TrackId::MAIN,
        device: suhina_engine::MIXER_VOLUME,
        param: 0,
        value: volume,
    }];
    Ok(json!({ "volume_db": to_db(volume) }))
}
