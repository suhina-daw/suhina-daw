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
