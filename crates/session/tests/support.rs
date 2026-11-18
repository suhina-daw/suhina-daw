// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the session's test files share: running a command on a session or on the open projects.

use serde_json::Value;
use suhina_session::{CommandError, Projects, Session};

/// Something commands run on: one session, or the projects open in the app.
pub trait Execute {
    fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError>;
}

impl Execute for Session {
    fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        Session::execute(self, id, params)
    }
}

impl Execute for Projects {
    fn execute(&mut self, id: &str, params: &Value) -> Result<Value, CommandError> {
        Projects::execute(self, id, params)
    }
}

/// Runs command `id` with `params`, its error as text.
pub fn run(target: &mut impl Execute, id: &str, params: Value) -> Result<Value, String> {
    target.execute(id, &params).map_err(String::from)
}

/// A command's result without the ids of the notes it names, which come from a counter for the
/// whole program (#1182).
pub fn without_ids(result: &Value) -> Value {
    let mut result = result.clone();
    if let Some(map) = result.as_object_mut() {
        map.remove("ids");
    }
    result
}

/// A session whose tracks with no instrument play their notes as sine tones (#1384), as every
/// session did before the setting: for the tests that hear notes on such tracks.
pub fn toned(project: suhina_model::Project) -> suhina_session::Session {
    let mut session = suhina_session::Session::new(project);
    session.set_implicit_tones(true);
    session
}
