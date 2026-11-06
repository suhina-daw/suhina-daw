// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What an agent may do through the MCP server (#764 › 12): the commands it may not run, the
//! file a save without a path writes, and the folders a call may read and write.

use super::*;

/// The file `project.save` without a path would write, and whose folder `project.autosave` and
/// `project.collect` without a folder write into: the source of the project it runs in. `None` for every other command.
pub(super) fn save_source(projects: &mut Projects, target: Option<ProjectId>, id: &str) -> Option<PathBuf> {
    if !matches!(id, "project.save" | "project.autosave" | "project.collect") {
        return None;
    }
    projects.session_mut(target).ok()?.source().map(Path::to_path_buf)
}

/// Why an agent may not run command `id` with `params`, if it may not; `attached` when it works
/// on the app's open tabs.
pub(super) fn refused(id: &str, params: Option<&Value>, attached: bool) -> Option<String> {
    match suhina_session::policy(id).map(|p| p.agents) {
        Some(Agents::Denied(why)) => return Some(format!("{id} is not available to agents: {why}")),
        Some(Agents::NotWhenAttached(why)) if attached => {
            // A reason that is a whole sentence of its own is given as it is.
            return Some(if why == Agents::SETTINGS { format!("{id} {why}") } else { why.to_string() });
        }
        _ => {}
    }
    if !attached {
        return None;
    }
    let sets_backups = params.is_some_and(|p| p.get("interval").is_some() || p.get("cleanUp").is_some());
    if id == "project.autosave" && sets_backups {
        return Some(
            "the backup settings are the person's; agents attached to the app may only back up now (now: true)".into(),
        );
    }
    None
}

/// The file rules a call follows.
#[derive(Clone)]
pub(super) enum Files {
    /// The roots `suhina-cli mcp` was given at launch.
    Fixed(Arc<Workspace>),
    /// Attached to the app: the folder of the project the call acts on, and the library to read.
    PerProject,
}

impl Files {
    pub(super) fn workspace(&self, projects: &mut Projects, target: Option<ProjectId>) -> Workspace {
        match self {
            Files::Fixed(workspace) => Workspace::clone(workspace),
            Files::PerProject => {
                let folder = projects
                    .session_mut(target)
                    .ok()
                    .and_then(|s| s.source().and_then(Path::parent).map(Path::to_path_buf))
                    .filter(|f| !f.as_os_str().is_empty());
                let library: Vec<PathBuf> = projects.file_library_mut().folders().into_iter().map(|f| f.path).collect();
                let (include_project, folders) = projects.mcp_write_folders();
                Workspace::attached(folder.as_deref(), &library).with_write_folders(include_project, folders)
            }
        }
    }
}
