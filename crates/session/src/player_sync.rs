// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`PlayerSync`]: the player a session plays through, and what an edit sends it.

use crate::{ParamEdit, Player};

/// The attached player, and how the last edit reaches it: parameter and fader messages, a new
/// graph, or nothing.
#[derive(Default)]
pub(crate) struct PlayerSync {
    pub(crate) player: Option<Player>,
    /// Set by an edit command whose changes are device parameters.
    pub(crate) param_edits: Vec<ParamEdit>,
    /// The fader moves of an edit that is only track volumes: sent to a playing engine as
    /// messages ([`suhina_engine::MIXER_VOLUME`]) instead of a new graph when it can take them.
    /// Unlike `param_edits`, they leave undo as the command's own steps make it.
    pub(crate) mixer_edits: Vec<ParamEdit>,
    /// Set by a command whose edit changes nothing the engine plays (a send of a muted track):
    /// the playing graph is kept, as for a command whose [`crate::Policy::heard`] is false.
    pub(crate) unheard: bool,
    /// Set when a continued edit could not reach the playing engine;
    /// [`crate::Session::catch_up_player`] sends the project again.
    pub(crate) behind: bool,
}
