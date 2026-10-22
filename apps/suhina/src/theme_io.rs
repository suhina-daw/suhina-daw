// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One owner of theme files, off the window thread. Only one batch is in flight;
//! later slider samples replace queued saves, without crossing rename/export barriers.
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui;
use suhina_ui::settings::themes::{FileAction, ThemeEntry};

use crate::theme_files::ThemeFiles;

struct Work {
    actions: Vec<FileAction>,
    repaint: Option<egui::Context>,
}

#[derive(Default)]
pub struct Reply {
    pub entries: Vec<ThemeEntry>,
    pub replace: bool,
    pub folder: Option<String>,
    pub select: Option<String>,
    pub notices: Vec<Result<String, String>>,
}

pub struct ThemeIo {
    send: Option<Sender<Work>>,
    replies: Receiver<Reply>,
    worker: Option<JoinHandle<()>>,
    pending: Vec<FileAction>,
    busy: bool,
    next_poll: Instant,
    folder: Option<String>,
}

impl ThemeIo {
    pub fn new(mut files: ThemeFiles) -> Result<Self, String> {
        let folder = files.folder_text();
        let (send, receive) = mpsc::channel::<Work>();
        let (done, replies) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("suhina-theme-files".into())
            .spawn(move || {
                while let Ok(work) = receive.recv() {
                    let reply = suhina_plugins::trace::timed("theme file worker", || execute(&mut files, work.actions));
                    if done.send(reply).is_err() {
                        break;
                    }
                    if let Some(ctx) = work.repaint {
                        ctx.request_repaint();
                    }
                }
            })
            .map_err(|error| format!("could not start theme file worker: {error}"))?;
        Ok(Self {
            send: Some(send),
            replies,
            worker: Some(worker),
            pending: Vec::new(),
            busy: false,
            next_poll: Instant::now(),
            folder,
        })
    }

    pub fn folder_text(&self) -> Option<String> {
        self.folder.clone()
    }

    pub fn queue(&mut self, action: FileAction) {
        if let FileAction::Save { id, .. } = &action {
            for previous in self.pending.iter_mut().rev() {
                match previous {
                    FileAction::Save { id: old, .. } if old == id => {
                        *previous = action;
                        return;
                    }
                    FileAction::Save { .. } => {}
                    _ => break,
                }
            }
        }
        self.pending.push(action);
    }

    pub fn tick(&mut self, ctx: &egui::Context) -> Option<Reply> {
        self.send.as_ref()?;
        let reply = match self.replies.try_recv() {
            Ok(mut reply) => {
                self.busy = false;
                // A completed poll must not replace a newer edit still queued locally.
                if reply.replace {
                    for action in &self.pending {
                        match action {
                            FileAction::Save { id, theme } => {
                                reply.entries.retain(|entry| entry.id != *id);
                                reply.entries.push(ThemeEntry { id: id.clone(), theme: theme.as_ref().clone() });
                            }
                            FileAction::Rename { from, to } => {
                                for entry in &mut reply.entries {
                                    if entry.id == *from {
                                        entry.id.clone_from(to);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    reply.entries.retain(|entry| {
                        !self.pending.iter().any(|action| match action {
                            FileAction::Save { id, .. } => *id == entry.id,
                            FileAction::Rename { from, to } => *from == entry.id || *to == entry.id,
                            _ => false,
                        })
                    });
                }
                Some(reply)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.send.take();
                return Some(Reply { notices: vec![Err("theme file worker stopped".into())], ..Default::default() });
            }
        };
        if !self.busy && (!self.pending.is_empty() || Instant::now() >= self.next_poll) {
            let work = Work { actions: std::mem::take(&mut self.pending), repaint: Some(ctx.clone()) };
            if let Some(send) = &self.send {
                match send.send(work) {
                    Ok(()) => {
                        self.busy = true;
                        self.next_poll = Instant::now() + Duration::from_secs(1);
                    }
                    Err(error) => {
                        self.pending = error.0.actions;
                        self.send.take();
                        let mut reply = reply.unwrap_or_default();
                        reply.notices.push(Err("theme file worker stopped before saving".into()));
                        return Some(reply);
                    }
                }
            }
        }
        // The worker wakes the window when its reply is ready. Polling a busy
        // disk at frame rate would needlessly keep liquid/GPU rendering alive.
        if !self.busy {
            // egui wakes a predicted frame early; leave headroom so that an idle
            // window does not draw repeatedly just before the poll deadline.
            ctx.request_repaint_after(
                self.next_poll.saturating_duration_since(Instant::now()) + Duration::from_millis(50),
            );
        }
        reply
    }

    /// Drain the last coalesced edits on normal exit, before the process can end the worker.
    pub fn finish(&mut self) {
        if let Some(send) = self.send.take() {
            if !self.pending.is_empty() {
                let _sent = send.send(Work { actions: std::mem::take(&mut self.pending), repaint: None });
            }
            drop(send);
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                eprintln!("theme file worker failed while saving");
            }
            for reply in self.replies.try_iter() {
                for error in reply.notices.into_iter().filter_map(Result::err) {
                    eprintln!("{error}");
                }
            }
        }
    }
}

impl Drop for ThemeIo {
    fn drop(&mut self) {
        self.finish();
    }
}

fn execute(files: &mut ThemeFiles, actions: Vec<FileAction>) -> Reply {
    let mut reply = Reply { folder: files.folder_text(), ..Default::default() };
    let mut reload = false;
    for action in actions {
        let result = match action {
            FileAction::Save { id, theme } => files.write(&id, &theme).map(|()| None),
            FileAction::Rename { from, to } => files.rename(&from, &to).map(|()| None),
            FileAction::Reload => {
                reply.replace = true;
                reload = true;
                Ok(None)
            }
            FileAction::OpenFolder => files.open_folder().map(|()| None),
            FileAction::Backup => files.backup().map(|path| Some(crate::saved(&path, "Backed up the themes to"))),
            FileAction::ExportTemplate => {
                files.export_template().map(|path| Some(crate::saved(&path, "Saved the theme template to")))
            }
            FileAction::Import(path) => files.import(&path).map(|(ids, warnings)| {
                reply.replace = true;
                if let [id] = ids.as_slice() {
                    reply.select = Some(id.clone());
                }
                let file = path.file_name().unwrap_or_default().to_string_lossy();
                let message = match ids.len() {
                    0 => format!("{file} has no themes that are not here already"),
                    1 => format!("Added the theme from {file}"),
                    n => format!("Added {n} themes from {file}"),
                };
                Some(warnings.first().map_or(message.clone(), |warning| format!("{message}; {warning}")))
            }),
        };
        match result {
            Ok(Some(message)) => reply.notices.push(Ok(message)),
            Ok(None) => {}
            Err(error) => reply.notices.push(Err(error)),
        }
    }
    let (entries, warnings) = if reply.replace { files.read_all() } else { files.changed() };
    if reload && warnings.is_empty() {
        reply.notices.push(Ok(format!("Read {} theme files", entries.len())));
    }
    reply.entries = entries;
    reply.notices.extend(warnings.into_iter().map(Err));
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_ui::kit::theme::Theme;

    fn controlled() -> (ThemeIo, Receiver<Work>, Sender<Reply>) {
        let (send, receive) = mpsc::channel();
        let (done, replies) = mpsc::channel();
        (
            ThemeIo {
                send: Some(send),
                replies,
                worker: None,
                pending: Vec::new(),
                busy: false,
                next_poll: Instant::now(),
                folder: None,
            },
            receive,
            done,
        )
    }

    #[test]
    fn a_failed_worker_reports_the_unsaved_batch() {
        let (mut io, receive, _done) = controlled();
        drop(receive);
        io.queue(FileAction::Save { id: "test".into(), theme: Box::new(Theme::default()) });
        let reply = io.tick(&egui::Context::default()).unwrap();
        assert!(reply.notices.iter().any(Result::is_err));
        assert_eq!(io.pending.len(), 1, "keep the failed batch available for diagnosis");
    }

    #[test]
    fn a_reload_reply_cannot_restore_a_name_edited_while_it_was_in_flight() {
        let (mut io, receive, done) = controlled();
        io.busy = true;
        done.send(Reply {
            replace: true,
            entries: vec![ThemeEntry { id: "old".into(), theme: Theme::default() }],
            ..Default::default()
        })
        .unwrap();
        io.queue(FileAction::Rename { from: "old".into(), to: "new".into() });
        io.queue(FileAction::Save {
            id: "new".into(),
            theme: Box::new(Theme { name: "Latest".into(), ..Default::default() }),
        });
        let reply = io.tick(&egui::Context::default()).unwrap();
        assert_eq!(reply.entries.len(), 1);
        assert_eq!(reply.entries[0].id, "new");
        assert_eq!(reply.entries[0].theme.name, "Latest");
        let _work = receive.recv().unwrap();
    }

    #[test]
    fn a_busy_writer_does_not_block_ticks_or_accumulate_slider_history() {
        let (send, receive) = mpsc::channel();
        let (done, replies) = mpsc::channel();
        let mut io = ThemeIo {
            send: Some(send),
            replies,
            worker: None,
            pending: Vec::new(),
            busy: false,
            next_poll: Instant::now(),
            folder: None,
        };
        let ctx = egui::Context::default();
        io.queue(FileAction::Save { id: "test".into(), theme: Box::new(Theme::default()) });
        assert!(io.tick(&ctx).is_none());
        let _in_flight = receive.recv().unwrap();
        // Hold the worker's reply while the actual window-facing API receives more input.
        for value in 1..=100 {
            let theme = Theme { name: format!("value-{value}"), ..Default::default() };
            io.queue(FileAction::Save { id: "test".into(), theme: Box::new(theme) });
            assert!(io.tick(&ctx).is_none());
        }
        assert_eq!(io.pending.len(), 1);
        assert!(receive.try_recv().is_err(), "more than one batch in flight");
        done.send(Reply::default()).unwrap();
        assert!(io.tick(&ctx).is_some());
        let next = receive.recv().unwrap();
        assert!(matches!(next.actions.as_slice(), [FileAction::Save { theme, .. }] if theme.name == "value-100"));
    }

    #[test]
    fn slider_samples_coalesce_and_exit_saves_the_final_value() {
        let root = std::env::temp_dir().join(format!("suhina-theme-worker-{}", std::process::id()));
        let mut io = ThemeIo::new(ThemeFiles::new(Some(root.clone()))).unwrap();
        for value in 1..=100 {
            let theme = Theme { name: format!("sample-{value}"), ..Default::default() };
            io.queue(FileAction::Save { id: "test".into(), theme: Box::new(theme) });
        }
        assert_eq!(io.pending.len(), 1);
        assert!(!root.exists(), "queueing must not do file I/O on the caller");
        io.finish();
        assert!(std::fs::read_to_string(root.join("test.json")).unwrap().contains("sample-100"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_rename_is_a_barrier_between_coalesced_saves() {
        let mut io = ThemeIo::new(ThemeFiles::new(None)).unwrap();
        let save = || FileAction::Save { id: "test".into(), theme: Box::new(Theme::default()) };
        io.queue(save());
        io.queue(FileAction::Rename { from: "test".into(), to: "other".into() });
        io.queue(save());
        assert_eq!(io.pending.len(), 3);
        io.pending.clear();
    }
}
