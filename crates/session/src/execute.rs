// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Session::execute`] in three steps (#764 › 2): [`Session::prepare`] checks the parameters
//! and works out which undo step and which drag the command belongs to; the command runs, and
//! [`Session::apply`] gives what it added ids and checks the project; [`Session::commit`] is the
//! only step that records undo and tells the player.

use serde_json::Value;
use suhina_model::Project;

use crate::commands::Args;
use crate::commands::{self, CommandKind, CommandSpec, commands};
use crate::{CommandError, Session, unknown_command};

/// What [`Session::prepare`] works out before a command runs.
struct Prepared<'a> {
    spec: &'static CommandSpec,
    /// `params` with clips named by id resolved to where they are now, when any were.
    resolved: Option<Value>,
    /// The gesture the call names (#1146): one drag's calls make one undo step.
    gesture: Option<&'a str>,
    /// Whether the call joins the gesture that is open.
    joins: bool,
    /// A call that joins an open gesture of a command whose calls replace each other: it runs on
    /// the project as it was before the gesture began.
    replaces: bool,
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
            CommandKind::HISTORY => {
                let scenes = self.project.launcher.clone();
                let value = (spec.run)(self, params)?;
                let shift = suhina_engine::SceneShift::between(&scenes, &self.project.launcher);
                self.update_player(shift)?;
                // Undo may have changed what an armed track hears.
                self.send_armed();
                Ok(value)
            }
            CommandKind::EDIT => {
                let before = self.project.clone();
                // A live value that replaces the last one runs on the project as the step found
                // it (the top of the undo stack, which the open step pushed); `before` stays the
                // project as shown, so the player hears the change.
                if prepared.replaces
                    && let Some(start) = self.history.undo.last()
                {
                    self.project = start.clone();
                }
                match self.apply(&prepared, params, &before) {
                    Ok(value) => self.commit(&prepared, before).map(|()| value),
                    Err(e) => {
                        self.project = before;
                        Err(e)
                    }
                }
            }
            // Reads, transport, and what runs outside a project's undo history.
            _ => (spec.run)(self, params),
        }
    }

    /// Before a command runs: finds it, checks its parameters, resolves the clips they name by
    /// id, and for an edit, works out which undo step and which drag it belongs to.
    fn prepare<'a>(&mut self, id: &str, params: &'a Value) -> Result<Prepared<'a>, CommandError> {
        let spec = commands().iter().find(|c| c.id == id).ok_or_else(|| unknown_command(id))?;
        // A command that takes `gesture` makes one undo step of a drag's calls (#1146).
        let gesture = match spec.params.iter().any(|p| p.name == "gesture") {
            true => gesture_of(params)?,
            false => None,
        };
        commands::check_params(id, spec.params, params)?;
        // A clip named by its id is found where it is now and handed to the command by position.
        let resolved = commands::resolve_clip_ids(&self.project, spec, params)?;
        let mut prepared = Prepared { spec, resolved, gesture, joins: false, replaces: false };
        match spec.kind {
            // Undo and redo end the gesture first, then undo or redo it as one step.
            CommandKind::HISTORY => self.history.gesture = None,
            CommandKind::EDIT => {
                self.sync.param_edits.clear();
                self.sync.mixer_edits.clear();
                self.sync.unheard = false;
                prepared.joins = gesture.is_some() && self.history.gesture.as_deref() == gesture;
                if !prepared.joins {
                    // Any other edit, or a call with another gesture, ends the one that is open.
                    self.history.gesture = None;
                }
                prepared.replaces = prepared.joins && spec.policy.gesture_replaces;
            }
            _ => {}
        }
        Ok(prepared)
    }

    /// `history.endGesture {gesture}`: closes the gesture if it is the open one. Whether it was.
    pub(crate) fn end_gesture(&mut self, gesture: &str) -> bool {
        let open = self.history.gesture.as_deref() == Some(gesture);
        if open {
            self.history.gesture = None;
        }
        open
    }

    /// Runs an edit on the project, gives what it added an id and keeps the tracks' colours, and
    /// fails when what is left does not validate. The caller puts `before` back on an error.
    fn apply(&mut self, prepared: &Prepared, params: &Value, before: &Project) -> Result<Value, CommandError> {
        let mut value = (prepared.spec.run)(self, params)?;
        if prepared.spec.id.starts_with("device.") {
            crate::devices::follow_instruments(self, before);
            crate::clip_ara::keep_hosts_first(&mut self.project);
        }
        // A lane, clip, scene or device the command added without an id gets one now.
        self.project.assign_ids()?;
        // A macro mapping whose device was removed or left its group goes with it.
        self.project.drop_lost_mappings();
        if prepared.spec.id.starts_with("device.") {
            crate::devices::name_new_devices(&self.project, params, before, &mut value);
        }
        self.keep_track_colours(before);
        if matches!(prepared.spec.id, "clip.create" | "clip.addFiles") {
            self.colour_new_clips(before);
        }
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
            // The first call of a gesture, or an edit outside one, makes a step; a call that joins
            // the open gesture adds to its step. The gesture opens with its first change.
            if prepared.joins {
                self.history.redo.clear();
            } else {
                self.push_undo(before);
            }
            self.history.gesture = prepared.gesture.map(str::to_owned);
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
                match self.follow_player(shift) {
                    Ok(()) => self.sync.behind = false,
                    // A drag sends a value every frame and may outrun the engine taking new
                    // graphs: the engine catches up on a later frame. Any other failure (a graph
                    // that cannot be built) is the edit's error, as outside a gesture.
                    Err(lag) if lag.transient && prepared.gesture.is_some() => self.sync.behind = true,
                    Err(lag) => return Err(lag.error),
                }
                // The edit may have changed what an armed track hears.
                self.send_armed();
            }
        }
        Ok(())
    }
}

/// The `gesture` of a call: 1 to 128 bytes of text, or none.
fn gesture_of(params: &Value) -> Result<Option<&str>, CommandError> {
    match Args::new(params).value("gesture") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 128 => Ok(Some(s.as_str())),
        Some(_) => Err("gesture must be 1 to 128 bytes of text".into()),
    }
}
