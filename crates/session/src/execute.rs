// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Session::execute`] in three steps (#764 › 2): [`Session::prepare`] checks the parameters
//! and works out which undo step and which drag the command belongs to; the command runs, and
//! [`Session::apply`] gives what it added ids and checks the project; [`Session::commit`] is the
//! only step that records undo and tells the player.

use serde_json::Value;
use suhina_model::{Project, TrackId};

use crate::commands::{self, CommandKind, CommandSpec, commands};
use crate::{CommandError, Session, unknown_command};

/// What [`Session::prepare`] works out before a command runs.
struct Prepared<'a> {
    spec: &'static CommandSpec,
    /// `params` with clips named by id resolved to where they are now, when any were.
    resolved: Option<Value>,
    /// The key a `device.setParam` drag gives its gesture, so its calls make one undo step.
    gesture: Option<&'a str>,
    /// A `device.setParam` call that is one frame of a drag.
    continuous: bool,
    /// The undo step a `continue` call may join: the command and its track.
    step: Option<(&'static str, TrackId)>,
    /// Whether this call asked to `continue` the step.
    continued: bool,
}

impl Session {
    /// Runs the command `id` with JSON `params` and returns its JSON result. An editing command
    /// records one undo step when it changes the project; if it fails, or leaves a project that
    /// does not validate, the project is restored and an error is returned.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        let prepared = self.prepare(id, params)?;
        let params = prepared.resolved.as_ref().unwrap_or(params);
        let spec = prepared.spec;
        match spec.kind {
            CommandKind::Read
            | CommandKind::Transport
            | CommandKind::Projects
            | CommandKind::Plugins
            | CommandKind::Library
            | CommandKind::Audio
            | CommandKind::Plugin => (spec.run)(self, params),
            CommandKind::History => {
                self.history.open_step = None;
                let scenes = self.project.launcher.clone();
                let value = (spec.run)(self, params)?;
                let shift = suhina_engine::SceneShift::between(&scenes, &self.project.launcher);
                self.update_player(shift)?;
                // Undo may have changed what an armed track hears.
                self.send_armed();
                Ok(value)
            }
            CommandKind::Edit => {
                let before = self.project.clone();
                match self.apply(&prepared, params, &before) {
                    Ok(value) => self.commit(&prepared, before).map(|()| value),
                    Err(e) => {
                        self.project = before;
                        Err(e)
                    }
                }
            }
        }
    }

    /// Before a command runs: finds it, checks its parameters, resolves the clips they name by
    /// id, and for an edit, works out which undo step and which drag it belongs to.
    fn prepare<'a>(&mut self, id: &str, params: &'a Value) -> Result<Prepared<'a>, CommandError> {
        let spec = commands().iter().find(|c| c.id == id).ok_or_else(|| unknown_command(id))?;
        let gesture = if id == "device.setParam" {
            match params.get("gesture") {
                None => None,
                Some(Value::String(s)) if !s.is_empty() && s.len() <= 128 => Some(s.as_str()),
                Some(_) => return Err("gesture must be 1..128 bytes of text".into()),
            }
        } else {
            None
        };
        let continuous = id == "device.setParam" && params.get("continuous").and_then(Value::as_bool) == Some(true);
        if (spec.kind == CommandKind::Edit && !continuous && gesture.is_none()) || spec.kind == CommandKind::History {
            self.history.param_gesture = None;
        }
        commands::check_params(id, spec.params, params)?;
        // A clip named by its id is found where it is now and handed to the command by position.
        let resolved = commands::resolve_clip_ids(&self.project, spec, params)?;
        let mut prepared = Prepared { spec, resolved, gesture, continuous, step: None, continued: false };
        if spec.kind == CommandKind::Edit {
            let params = prepared.resolved.as_ref().unwrap_or(params);
            self.sync.param_edits.clear();
            self.sync.mixer_edits.clear();
            self.sync.unheard = false;
            // A command with a `continue` parameter can add to the step it made last time.
            prepared.step = spec
                .params
                .iter()
                .any(|p| p.name == "continue")
                .then(|| self.step_track(params))
                .flatten()
                .map(|track| (spec.id, track));
            prepared.continued = params.get("continue").and_then(Value::as_bool) == Some(true);
            if !prepared.continued || prepared.step != self.history.open_step {
                self.history.open_step = None;
            }
        }
        Ok(prepared)
    }

    /// The track a `continue` step is kept for: the command's track, the first of several, or for
    /// a command with no track (the main output's), a mark of its own.
    fn step_track(&self, params: &Value) -> Option<TrackId> {
        commands::track_id(&self.project, params)
            .ok()
            .or_else(|| commands::track_list(&self.project, params).ok().and_then(|l| l.first().copied()))
            .or_else(|| {
                (!params.as_object().is_some_and(|o| o.contains_key("track") || o.contains_key("tracks")))
                    .then_some(TrackId(u32::MAX))
            })
    }

    /// Runs an edit on the project, gives what it added an id and keeps the tracks' colours, and
    /// fails when what is left does not validate. The caller puts `before` back on an error.
    fn apply(&mut self, prepared: &Prepared, params: &Value, before: &Project) -> Result<Value, CommandError> {
        let value = (prepared.spec.run)(self, params)?;
        // A lane, clip or scene the command added without an id gets one now.
        self.project.assign_ids()?;
        self.keep_track_colours(before);
        match self.project.validate() {
            Ok(()) => Ok(value),
            Err(errors) => {
                Err(CommandError(format!("{} would leave an invalid project: {errors:?}", prepared.spec.id)))
            }
        }
    }

    /// After an edit that went through: records the undo step it makes (or joins the step its drag
    /// is making), and sends the change to the player as parameter or fader messages, or as a new
    /// graph. The only step that touches undo and the player.
    fn commit(&mut self, prepared: &Prepared, before: Project) -> Result<(), CommandError> {
        if self.project != before {
            let shift = suhina_engine::SceneShift::between(&before.launcher, &self.project.launcher);
            self.history.changes = self.history.changes.wrapping_add(1);
            let changes = std::mem::take(&mut self.sync.param_edits);
            let gesture = changes.first().map(|change| {
                (
                    change.track,
                    change.device,
                    prepared
                        .gesture
                        .map_or_else(|| format!("parameter:{}", change.param), |key| format!("custom:{key}")),
                )
            });
            if !changes.is_empty() {
                if (!prepared.continuous && prepared.gesture.is_none()) || self.history.param_gesture != gesture {
                    self.push_undo(before);
                }
                self.history.param_gesture = prepared.continuous.then_some(gesture).flatten();
            } else if self.history.open_step.is_some() {
                self.history.redo.clear();
            } else {
                self.push_undo(before);
                self.history.open_step = prepared.step;
            }
            // A parameter change goes to the engine as a message; anything else, or a message
            // that cannot be sent, as a new graph.
            let mixer = std::mem::take(&mut self.sync.mixer_edits);
            // An edit that changes nothing the engine plays (a colour, a name, a marker) keeps the
            // playing graph: colouring every clip of a large project built a graph for each of
            // them, six seconds in all.
            let unheard = !prepared.spec.policy.heard || std::mem::take(&mut self.sync.unheard);
            let sent = unheard
                || match (changes.is_empty(), mixer.is_empty(), self.sync.player.as_mut()) {
                    (false, _, Some(player)) => {
                        changes.into_iter().all(|change| player.set_device_param(change).is_ok())
                    }
                    (true, false, Some(player)) => {
                        mixer.into_iter().all(|change| player.set_device_param(change).is_ok())
                    }
                    _ => false,
                };
            if !sent {
                match self.update_player(shift) {
                    Ok(()) => self.sync.behind = false,
                    // A drag sends a value every frame and may outrun the engine taking new
                    // graphs: the engine catches up on a later frame.
                    Err(_) if prepared.continued => self.sync.behind = true,
                    Err(e) => return Err(e),
                }
                // The edit may have changed what an armed track hears.
                self.send_armed();
            }
        }
        if !prepared.continuous {
            self.history.param_gesture = None;
        }
        Ok(())
    }
}
