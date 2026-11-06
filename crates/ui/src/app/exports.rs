// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! File › Export as a job: its data gathered over frames, rendered on a worker, and followed to the end.

use super::*;

impl App {
    /// File › Export…: the dialog filled with the project's own name and folder, a .wav beside
    /// its .suhina file.
    pub(super) fn open_export(&mut self) {
        let Some(id) = self.projects.active() else { return };
        let name = self.projects.name(id).unwrap_or("Untitled").to_string();
        let folder = self
            .own_file(id)
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.projects_folder());
        self.settings_open = false;
        let mut export = ExportDialog::new(folder.join(format!("{name}.wav")));
        // A restarted dialog over an export the worker is still running shows it, Exporting…;
        // over one still gathering its states the Cancel works (its close cancels the gather).
        export.busy = self.export.is_some() || self.export_gather.is_some();
        export.gathering = self.export_gather.is_some();
        self.export_dialog = Some(export);
    }

    /// Runs `project.export` on a worker, so the window keeps living while it renders and a
    /// plug-in that answers slowly cannot freeze it. The dialog stays up showing Exporting…;
    /// [`App::poll_export`] takes the result in.
    pub(super) fn start_export(&mut self, job: ExportJob) {
        let Some(id) = self.projects.active() else { return };
        // One export at a time: starting another would replace the one running's receiver and
        // its result would be lost. The guard holds whatever the dialog says — even one the
        // user closed while its worker was still rendering — and shows the running job again.
        if self.export.is_some() || self.export_gather.is_some() {
            self.status = Status::Error("an export is already running".into());
            if let Some(dialog) = self.export_dialog.as_mut() {
                dialog.busy = true;
            }
            return;
        }
        // The running plug-ins' states are gathered over this window's frames, never waited
        // for on its thread ([`App::gather_export`]): a plug-in that answers slowly keeps the
        // window drawing, and the export starts once the states are in, at most a second out.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        let Some(session) = self.projects.get_mut(id) else { return };
        match session.begin_export_gather(deadline) {
            Err(e) => self.status = Status::Error(e.message().to_owned()),
            Ok(ExportGather::Ready(data)) => {
                let name = self.projects.name(id).unwrap_or("the mix").to_string();
                self.launch_export(id, name, job, *data);
            }
            Ok(ExportGather::Pending) => {
                let name = self.projects.name(id).unwrap_or("the mix").to_string();
                self.export_gather = Some((id, job, deadline, name.clone()));
                if let Some(dialog) = self.export_dialog.as_mut() {
                    dialog.busy = true;
                    dialog.gathering = true;
                }
                self.status = Status::Hint(format!("Exporting {name}…"));
            }
        }
    }

    /// Pumps the gathering export once a frame: when the state reads are in (or their bounded
    /// wait is out), the export's worker starts with the whole data. The frames carry on the
    /// whole time; the waits the state read once made on the window's thread are gone.
    pub(super) fn gather_export(&mut self) {
        let Some((id, job, deadline, name)) =
            self.export_gather.as_ref().map(|(id, job, deadline, name)| (*id, job.clone(), *deadline, name.clone()))
        else {
            return;
        };
        let Some(session) = self.projects.get_mut(id) else {
            // The project was closed while the export gathered: the job stops, said with the
            // name it set out under, and nothing is lost silently. A later export starts over.
            self.export_gather = None;
            self.export_dialog = None;
            self.status = Status::Error(format!("the export of {name} stopped: the project closed"));
            return;
        };
        if let ExportGather::Ready(data) = session.gather_export_data(deadline) {
            self.export_gather = None;
            self.launch_export(id, name, job, *data);
        }
    }

    /// The export dialog closed: over a gathering export the close cancels the gather — its
    /// reads stand where they are, the late replies change nothing and start nothing; over a
    /// rendering one the worker goes on, its result still taken ([`App::poll_export`]).
    pub(super) fn cancel_export_gather(&mut self) {
        let Some((id, _, _, name)) = self.export_gather.take() else { return };
        if let Some(session) = self.projects.get_mut(id) {
            session.cancel_export_gather();
        }
        self.status = Status::Hint(format!("the export of {name} was cancelled"));
    }

    /// Spawns the export's worker with already gathered data ([`ExportData`](suhina_session::ExportData)), so the render,
    /// the encoding and every plug-in's part run off the window's thread. The dialogs stay up
    /// showing Exporting…; [`App::poll_export`] takes the result in. The feedback names the
    /// project the export belongs to, whatever tab shows after.
    pub(super) fn launch_export(
        &mut self,
        id: suhina_session::ProjectId,
        name: String,
        job: ExportJob,
        data: suhina_session::ExportData,
    ) {
        let params = job.params();
        let (done, receive) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("suhina-export".into()).spawn(move || {
            let _unheard = done.send(suhina_session::export_from(&data, &params).map_err(String::from));
        });
        match spawned {
            Ok(_) => {
                self.export = Some(receive);
                self.export_project = Some((id, name.clone()));
                if let Some(dialog) = self.export_dialog.as_mut() {
                    dialog.busy = true;
                    dialog.gathering = false;
                }
                self.status = Status::Hint(format!("Exporting {name}…"));
            }
            Err(e) => self.status = Status::Error(format!("could not start the export: {e}")),
        }
    }

    /// Picks up a finished export.
    pub(super) fn poll_export(&mut self, ctx: &egui::Context) {
        self.gather_export();
        if self.export_gather.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        let Some((_, name)) = self.export_project.as_ref().map(|(id, name)| (*id, name.clone())) else { return };
        let Some(receive) = self.export.take() else { return };
        match receive.try_recv() {
            Ok(result) => {
                self.status = match result {
                    Ok(summary) => {
                        self.export_dialog = None;
                        let where_to = summary["path"].as_str().unwrap_or("the file").to_string();
                        let warnings = summary["warnings"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(serde_json::Value::as_str)
                            .collect::<Vec<_>>()
                            .join("; ");
                        if warnings.is_empty() {
                            Status::Done(format!("Exported {name} to {where_to}"))
                        } else {
                            Status::Hint(format!("Exported {name} to {where_to}. Warnings: {warnings}"))
                        }
                    }
                    // A failed export keeps the dialog open with its settings.
                    Err(e) => {
                        if let Some(dialog) = self.export_dialog.as_mut() {
                            dialog.busy = false;
                            dialog.gathering = false;
                        }
                        Status::Error(format!("{name}: {e}"))
                    }
                };
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.export = Some(receive);
                ctx.request_repaint_after(std::time::Duration::from_millis(100))
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(dialog) = self.export_dialog.as_mut() {
                    dialog.busy = false;
                    dialog.gathering = false;
                }
                self.status = Status::Error(format!("the export of {name} stopped without a result"));
            }
        }
    }
}
