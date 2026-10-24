// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plug-ins a project runs (ADR 0006): for each plug-in device, the installed plug-in found
//! for it in the catalog, started in a host process with the state the project saved, off the
//! audio thread, and handed to the playing engine; its health watched, and a restart that gets
//! its last state back.
//!
//! The engine only sees each plug-in's audio half ([`Hosted`], a [`PluginProcessor`]); the
//! control half ([`Plugin`]) stays here, on the session's thread. A plug-in that is not
//! installed is *missing*: the device stays in the project with its state untouched, and the
//! engine passes the sound through it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use suhina_engine::{
    AudioPool, DeviceEvent, Followed, Joint, MAX_BLOCK_FRAMES, MAX_BLOCK_PARAMS, ParamEvent, PluginProcessor,
    PluginSlotInfo, Plugins, Sent, model::DeviceKind, plugin_devices,
};
use suhina_model::{Device, Project};
use suhina_plugins::library::{Hosting, group_for};
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_plugins::{
    Event, EventKind, Health, Launch, MAX_EVENTS, Placement, Plugin, PluginAudio, PluginError, SavedState,
};

use crate::plugin_edits::PluginEdits;
use crate::{CommandError, Player};

mod catalog;
mod hosted;
mod offline;

pub use self::catalog::*;
pub use self::hosted::*;
pub use self::offline::*;

/// How long starting a plug-in (the host process, loading it, its state) may take. Generous:
/// a plug-in that reads a large sample library as it loads can take half a minute or more
/// without being hung, and the loading dialog shows which plug-ins are still at it.
pub const START_TIMEOUT: Duration = Duration::from_secs(120);
/// How often the session looks whether its plug-ins still run.
const HEALTH_EVERY: Duration = Duration::from_millis(250);
/// How often the state of each running plug-in is read, so a restart after a crash gets back
/// what the user set in its editor (ADR 0006, decision 5).
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// How long saving a project waits for its plug-ins to give their state.
const SAVE_WAIT: Duration = Duration::from_secs(1);
/// MIDI controller numbers an all-notes-off is sent as.
/// How long a track's clips must stay put before a plug-in that edits them is loaded again
/// with them, so dragging a clip does not reload it at every step.
const REBIND_AFTER: Duration = Duration::from_millis(750);
const ALL_NOTES_OFF: u8 = 123;
const SUSTAIN: u8 = 64;

/// The host process group a plug-in runs in (`None`: a process of its own).
pub type ProcessGroup = Box<dyn Fn(&PluginInfo) -> Option<String> + Send + Sync>;

/// What the session needs to start plug-ins: the catalog of installed ones, the host
/// executable, and which plug-ins share a host process (ADR 0006, decision 2).
pub struct PluginEnv {
    pub catalog: Catalog,
    pub host: PathBuf,
    /// The process group of a plug-in (`None`: a process of its own).
    pub group: ProcessGroup,
}

impl PluginEnv {
    /// `catalog` with plug-ins run by `host`, every instance of one plug-in file in one process
    /// (the default).
    pub fn new(catalog: Catalog, host: PathBuf) -> Self {
        Self { catalog, host, group: Box::new(|plugin| group_for(Hosting::default(), plugin)) }
    }

    /// What `library` gives: its catalog without the plug-ins switched off, its host, and how
    /// each plug-in is hosted (its `plugin.setHosting` choice, else one process per plug-in
    /// file).
    pub fn from_library(library: &mut suhina_plugins::library::Library) -> Self {
        let host = library.host().to_path_buf();
        // Plug-ins switched off are neither offered nor loaded.
        let catalog = library.enabled_catalog();
        let chosen: HashMap<String, Hosting> = catalog
            .plugins()
            .map(PluginInfo::spec)
            .collect::<Vec<_>>()
            .into_iter()
            .map(|spec| {
                let hosting = library.hosting(&spec);
                (spec, hosting)
            })
            .filter(|(_, hosting)| *hosting != Hosting::default())
            .collect();
        let group: ProcessGroup =
            Box::new(move |plugin| group_for(chosen.get(&plugin.spec()).copied().unwrap_or_default(), plugin));
        Self { catalog, host, group }
    }
}

/// Where a plug-in device stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginStatus {
    /// Its host process is starting it.
    Loading,
    Running,
    /// No installed plug-in stands for it; it keeps its state and passes the sound through.
    Missing,
    /// Its host process exited or stopped answering; the engine plays on without it until it
    /// is restarted.
    Crashed(String),
    /// It could not be started.
    Failed(String),
}

impl PluginStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Running => "running",
            Self::Missing => "missing",
            Self::Crashed(_) => "crashed",
            Self::Failed(_) => "failed",
        }
    }

    /// Why it crashed or failed.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Crashed(why) | Self::Failed(why) => Some(why),
            _ => None,
        }
    }
}

/// One plug-in device's running plug-in, as the session sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginReport {
    pub status: PluginStatus,
    /// The installed plug-in that stands for it: name, vendor, how it was found.
    pub plugin: Option<(String, String, &'static str)>,
    pub instrument: bool,
    /// Frames of delay it reported.
    pub latency: u32,
    pub editor_open: bool,
    /// The open editor is embedded in the device panel of the session's window (rather than
    /// in a window of its own), and the size of its inside in pixels, as the host last said.
    pub editor_embedded: Option<(u32, u32)>,
    /// Blocks the engine gave it since it started, and how many it did not answer in time
    /// (the chain went on without it for that block).
    pub blocks: u32,
    pub late_blocks: u32,
    /// The longest a block's round trip took, in microseconds.
    pub longest_us: u32,
    /// Blocks that came with no time left before the deadline (an earlier plug-in used it).
    pub no_time: u32,
    /// Why it runs with its own defaults, when it did not take the saved state.
    pub state_refused: Option<String>,
    /// It is bound to an ARA document of its track's audio clips (ADR 0028), so its editor
    /// edits them.
    pub ara: bool,
    /// Its parameters as it last listed them, for its generated panel (#679); `None` until it
    /// has, or while it does not run.
    pub params: Option<Arc<[suhina_plugins::ParamInfo]>>,
}

/// How far a project's plug-ins have got with starting, for a dialog while they load.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PluginProgress {
    /// Plug-ins that are installed and should run.
    pub total: usize,
    pub ready: usize,
    /// The names of those still starting.
    pub loading: Vec<String>,
    /// Plug-in devices with no installed plug-in.
    pub missing: usize,
    /// The names of those that could not start or crashed.
    pub failed: Vec<String>,
}

/// A key for a rack's shared ARA documents, unique in this process
/// ([`suhina_plugins::protocol::AraDocument::document`]).
fn next_ara_document() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!("{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

/// The state the project saved for `device`, with a VST3 controller's own state.
fn saved_state(device: &Device) -> Option<SavedState> {
    let controller = device.extensions.0.iter().find(|e| e.key == "Vst3ControllerState").map(|e| e.data.clone());
    device.state.clone().map(|data| SavedState { data, controller })
}

/// A plug-in started off the session's thread.
struct Started {
    key: u64,
    /// Which start of the device this answers; an older one is dropped.
    generation: u64,
    result: Result<(Plugin, PluginAudio, Option<String>), String>,
    /// Its state read just after it started, for later reads to be compared with (a plug-in may
    /// report a state it was given in a form of its own); `None` when it did not answer.
    first_state: Option<SavedState>,
}

/// What a plug-in's control half did on a thread of its own, coming back.
enum Done {
    Opened(Result<(u32, u32), String>),
    Closed(Result<(), String>),
    /// Its state was read (now and then, for a restart, and to notice changes made in its
    /// editor); `None` when it did not answer.
    Saved(Option<SavedState>),
    /// It was stopped; its last state, for when it starts again.
    Stopped(Option<SavedState>),
    /// Its ARA document was brought in line with its track's clips.
    AraUpdated(Result<(), String>),
    /// Its parameters were listed for its generated panel; `None` when it did not answer.
    Listed(Option<Vec<suhina_plugins::ParamInfo>>),
    /// The edits of a clip carried to another track were stored ([`Carry`], by its number).
    AraExported(u64, Result<(String, Vec<u8>), String>),
    /// They were given to the other track's plug-in, or not yet.
    AraImported(u64, Result<(), String>),
}

/// How often a shown plug-in's parameters are listed again, for its generated panel to follow
/// changes made in its own editor.
const LIST_EVERY: Duration = Duration::from_secs(2);

/// How long a listing for the panel waits for the host. It lends out the plug-in's control half
/// meanwhile, so it must not wait for as long as the start-up timeout does: a host that stops
/// answering would keep restarts and health checks away for minutes.
const LIST_TIMEOUT: Duration = Duration::from_secs(2);

/// A parameter list's ids and ranges (lowest, highest), as [`Instance::ranges`] keeps them.
fn ranges(list: &[suhina_plugins::ParamInfo]) -> Vec<(u32, f64, f64)> {
    list.iter().map(|p| (p.id, p.min.min(p.max), p.max.max(p.min))).collect()
}

/// How long a clip's edits wait for the other track's plug-in to run with the clip.
const CARRY_FOR: Duration = Duration::from_secs(60);

/// A clip's ARA edits on their way from one track's plug-in to another's (ADR 0028 step 6): a
/// clip moved or copied there with its edits kept. Stored from the first (`from_key`) at once,
/// then given to the plug-in that edits clips on `to_track` once it runs with the clip.
pub(crate) struct Carry {
    pub(crate) number: u64,
    pub(crate) from_key: u64,
    pub(crate) to_track: suhina_model::TrackId,
    /// The modifications' persistent ids there and here (`content-<id>`), the source's (the
    /// pool key), and the clip's region here (`clip-<id>`), which says the clip has arrived.
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) source: String,
    pub(crate) region: String,
    pub(crate) archive: Option<(String, Vec<u8>)>,
    /// Out to a plug-in now (stored or given): nothing more is asked until it answers.
    pub(crate) busy: bool,
    pub(crate) since: Instant,
}

/// A control half back from a thread, or (stopped) only what it left.
struct Returned {
    key: u64,
    generation: u64,
    control: Option<Plugin>,
    done: Done,
}

/// One state-read round of [`Rack::gather_states`], tracked independently of the live
/// instances: a project's stop while a read is out takes the instances with it, and the
/// round's answers, its fallbacks, the not-answered's names and its completion outlive them.
struct StateGather {
    /// The keys the reads were lent to.
    out: Vec<u64>,
    /// The keys asked for whose control half was out at the launch (a plug-in's editor
    /// closing): their reads go out when the control comes back, and the deadline settles
    /// them with their fallback when it never does.
    deferred: Vec<u64>,
    /// The ones whose answer came back ([`Rack::take_returned`]).
    back: std::collections::HashSet<u64>,
    /// The answers themselves.
    states: HashMap<u64, SavedState>,
    /// What each asked plug-in's state stood at, at the launch: the fallback of the ones
    /// that do not answer.
    fallback: HashMap<u64, SavedState>,
    /// Each asked plug-in's name, as it stood at the launch: the not-answered's report
    /// names it, even after its instance is gone.
    names: HashMap<u64, String>,
}

struct Instance {
    status: PluginStatus,
    plugin: Option<(String, String, &'static str)>,
    /// The plug-in it runs ([`PluginInfo::spec`](suhina_plugins::scan::PluginInfo::spec)), empty
    /// before one is found.
    spec: String,
    info: PluginSlotInfo,
    /// `None` while it starts, or while a thread of its own talks to it.
    control: Option<Plugin>,
    /// Last completed state read, retained while the control half is on a worker.
    last_state: Option<SavedState>,
    generation: u64,
    state_refused: Option<String>,
    /// Whether its editor is open, as last known.
    editor_open: bool,
    /// The track whose clips it edits, for an ARA plug-in; and how far its analysis of them has
    /// come, 0 to 1, while it runs ([`Rack::ara_activity`]).
    track: Option<suhina_model::TrackId>,
    ara_progress: Option<f32>,
    /// How the editor was opened: embedded in the session's window, or in a window of its
    /// own. Set when it is asked to open, so a placement that comes while it opens is kept.
    editor_embedded: bool,
    /// The size of the open editor's inside in pixels, as last known.
    editor_size: Option<(u32, u32)>,
    /// Where the window wants the embedded editor, and what the host was last told.
    placement_wanted: Option<Placement>,
    placement_sent: Option<Placement>,
    /// Sends the embedded editor's placement and size even while the control half is lent out
    /// (an ARA edit, an editor opening), so the editor never lags behind the window.
    placer: Option<suhina_plugins::EditorPlacer>,
    /// An editor asked open (`true`) or closed while the control half was out: done when it is
    /// back, rather than refused as busy.
    editor_wanted: Option<bool>,
    /// The room in pixels the window has for its embedded editor, where the editor is asked to
    /// fit it (an ARA editor in the clip's place), and what the host was last asked.
    fit_wanted: Option<(u32, u32)>,
    fit_sent: Option<(u32, u32)>,
    /// The editor is being closed to open again the other way (the embedding setting changed).
    reopen_editor: bool,
    /// An embedded editor cannot open on this system (macOS: a view of the host's process
    /// cannot attach to another process's window), so editors open in a window of their own
    /// instead, whatever the setting says.
    force_floating: bool,
    /// How it keeps up with the engine, while its audio half runs there.
    counts: Option<Arc<BlockCounts>>,
    /// Its parameters' ids and ranges (lowest, highest), once listed ([`Rack::params`]).
    ranges: Option<Vec<(u32, f64, f64)>>,
    /// Its parameters as last listed in the background for its generated panel, and when.
    listed: Option<(Arc<[suhina_plugins::ParamInfo]>, Instant)>,
    /// A listing for the panel is under way.
    listing: bool,
    /// When the last listing for the panel ended, answered or not; a plug-in that refuses to
    /// list is asked as seldom as one that answers.
    list_tried: Option<Instant>,
    /// For a plug-in that edits clips (ADR 0028): its track's clips as it was loaded with them.
    ara_layout: Option<crate::ara::Layout>,
    /// The audio sources of the ARA document it was loaded with, kept for as long as it runs;
    /// empty without a document.
    /// By audio pool key; a file's samples go there once and stay while it runs.
    ara_sources: Vec<(String, suhina_plugins::AudioSource)>,
    /// Since when its track's clips differ from its layout, for a plug-in loaded without a
    /// document (its track had no audio clips): it is loaded again with them once they have
    /// stayed put for [`REBIND_AFTER`]. One bound to a document is updated at once instead.
    ara_changed: Option<Instant>,
    /// An update of its document failed: it is loaded again with the clips instead.
    ara_failed: bool,
    /// Its ARA editor was taken out of the window to float in one of its own ([`Rack::set_ara_floating`]);
    /// otherwise an ARA plug-in's editor opens embedded whatever the embedding setting says.
    ara_float: bool,
    /// The region its ARA editor was last told is selected (`None`: not told yet).
    ara_selection_sent: Option<Option<String>>,
}

impl Instance {
    /// Whether its parameters are to be listed again for the panel: it runs, no listing is under
    /// way and the last one, answered or not, ended [`LIST_EVERY`] ago or never.
    fn list_due(&self, now: Instant) -> bool {
        self.status == PluginStatus::Running
            && !self.listing
            && self.list_tried.is_none_or(|at| now.saturating_duration_since(at) >= LIST_EVERY)
    }

    /// A listing for the panel ended at `now` with `list`, or with nothing when the plug-in did
    /// not answer in time; the old list stays then.
    fn list_done(&mut self, list: Option<Vec<suhina_plugins::ParamInfo>>, now: Instant) {
        self.listing = false;
        self.list_tried = Some(now);
        if let Some(list) = list {
            self.ranges = Some(ranges(&list));
            self.listed = Some((list.into(), now));
        }
    }

    fn new(status: PluginStatus, plugin: Option<(String, String, &'static str)>, generation: u64) -> Self {
        Self {
            status,
            plugin,
            spec: String::new(),
            info: PluginSlotInfo::default(),
            ranges: None,
            listed: None,
            listing: false,
            list_tried: None,
            control: None,
            last_state: None,
            generation,
            state_refused: None,
            editor_open: false,
            track: None,
            ara_progress: None,
            editor_embedded: false,
            editor_size: None,
            placement_wanted: None,
            placement_sent: None,
            placer: None,
            editor_wanted: None,
            fit_wanted: None,
            fit_sent: None,
            reopen_editor: false,
            force_floating: false,
            counts: None,
            ara_layout: None,
            ara_sources: Vec::new(),
            ara_changed: None,
            ara_failed: false,
            ara_float: false,
            ara_selection_sent: None,
        }
    }

    /// The sender of the embedded editor's placement, taken from the control half the first time
    /// it is here; kept while the control half is lent out.
    fn placer(&mut self) -> Option<&suhina_plugins::EditorPlacer> {
        if self.placer.is_none() {
            self.placer = self.control.as_mut().and_then(suhina_plugins::Plugin::placer);
        }
        self.placer.as_ref()
    }

    /// Sends the embedded editor its placement now; `false` when it could not be sent (the
    /// host gone: the placer is let go, and taken again from the next control half).
    fn send_placement(&mut self, placement: Placement) -> bool {
        let Some(placer) = self.placer() else {
            suhina_plugins::trace!("placement waits (no placer): {placement:?}");
            return false;
        };
        let sent = placer.place(placement);
        suhina_plugins::trace!("placement sent {sent}: {placement:?}");
        if !sent {
            self.placer = None;
        }
        sent
    }

    /// Asks the embedded editor to take `fit` pixels now, as [`Instance::send_placement`].
    fn send_fit(&mut self, (width, height): (u32, u32)) -> bool {
        let Some(placer) = self.placer() else { return false };
        let sent = placer.fit(width, height);
        if !sent {
            self.placer = None;
        }
        sent
    }

    /// The editor is closed (or gone with its process).
    fn editor_gone(&mut self) {
        self.editor_open = false;
        self.editor_size = None;
        self.placement_wanted = None;
        self.placement_sent = None;
        self.fit_wanted = None;
        self.fit_sent = None;
    }
}

/// The plug-ins one project runs, while it plays.
pub struct Rack {
    env: Option<std::sync::Arc<PluginEnv>>,
    sample_rate: u32,
    instances: HashMap<u64, Instance>,
    /// The state reads an export gathers with ([`Rack::gather_states`]): a round tracked
    /// independently of the live instances, so a project's stop while a read is out — the
    /// read's answer, its fallback and the round's completion — is not lost with them.
    state_gather: Option<StateGather>,
    /// The last state of plug-ins that were stopped (a device removed, the project no longer
    /// playing), so they come back as they were.
    kept: HashMap<u64, SavedState>,
    sender: Sender<Started>,
    receiver: Receiver<Started>,
    next_generation: u64,
    checked: Instant,
    /// Deadline and read rounds left for a nonblocking quit check.
    edit_check: Option<(Instant, u8)>,
    saved: Instant,
    /// How often running plug-ins' states are read: [`SAVE_EVERY`], shorter in tests.
    save_every: Duration,
    /// The window editors open over (owned by it), when the session runs in one.
    owner: Option<u64>,
    /// Editors open embedded in that window's device panel rather than in windows of their own.
    embed: bool,
    returned_sender: Sender<Returned>,
    returned: Receiver<Returned>,
    /// What happened while the project was saved, for the next [`Rack::sync`] to tell.
    notices: Vec<String>,
    /// The states read compared with the ones last saved: changes made in plug-in editors.
    pub(crate) edits: PluginEdits,
    /// Plug-ins that edit clips, stopped to be loaded again with their track's new clips: each
    /// starts again once its state is back ([`Rack::follow_clips`]).
    rebinding: std::collections::HashSet<u64>,
    /// Of those, the ones whose editor was open: it opens again once the plug-in runs.
    reopen_after_rebind: std::collections::HashSet<u64>,
    /// Plug-in devices the host found to support ARA when they loaded although the catalog did
    /// not say so (a scan that missed it): they are loaded again with their track's clips, and
    /// from then on as plug-ins that edit clips.
    ara_found: std::collections::HashSet<u64>,
    /// The key of the ARA documents this rack's plug-ins share in their host processes: one a
    /// plug-in per project, each track a region sequence of it ([`suhina_plugins::protocol::AraDocument::document`]).
    ara_document: String,
    /// The plug-ins (by spec) found that way: the next device of one starts with its clips at
    /// once, rather than loading twice. Taken by [`Rack::take_ara_learned`] for the catalog.
    ara_plugins: std::collections::HashSet<String>,
    ara_learned: Vec<String>,
    /// The region each plug-in that edits clips should show as selected (`clip-<id>`), by
    /// plug-in device key; told to it whenever it runs ([`Rack::select_ara`]).
    ara_selection: HashMap<u64, Option<String>>,
    /// The notes ARA plug-ins found in the clips they edit, by clip (#720); none for a plug-in
    /// that is bypassed or not running.
    ara_notes: HashMap<suhina_model::ClipId, crate::ara::ClipNotes>,
    /// Of those, the clips whose plug-in is bypassed: kept, not shown (ADR 0028).
    ara_notes_hidden: std::collections::HashSet<suhina_model::ClipId>,
    /// Clips' edits on their way to another track's plug-in ([`Rack::carry`]).
    carries: Vec<Carry>,
    next_carry: u64,
}

impl Default for Rack {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        let (returned_sender, returned) = mpsc::channel();
        Self {
            returned_sender,
            returned,
            notices: Vec::new(),
            env: None,
            sample_rate: 0,
            instances: HashMap::new(),
            state_gather: None,
            kept: HashMap::new(),
            sender,
            receiver,
            next_generation: 0,
            checked: Instant::now(),
            edit_check: None,
            saved: Instant::now(),
            save_every: SAVE_EVERY,
            owner: None,
            embed: false,
            edits: PluginEdits::default(),
            rebinding: std::collections::HashSet::new(),
            reopen_after_rebind: std::collections::HashSet::new(),
            ara_selection: HashMap::new(),
            ara_notes: HashMap::new(),
            ara_notes_hidden: std::collections::HashSet::new(),
            ara_found: std::collections::HashSet::new(),
            ara_document: next_ara_document(),
            ara_plugins: std::collections::HashSet::new(),
            ara_learned: Vec::new(),
            carries: Vec::new(),
            next_carry: 0,
        }
    }
}

impl Rack {
    /// What plug-ins are started with. A new one (after a scan, or another hosting choice)
    /// makes the plug-ins that were not found, or could not start, be looked for again.
    pub fn set_env(&mut self, env: std::sync::Arc<PluginEnv>) {
        if self.env.as_ref().is_some_and(|old| std::sync::Arc::ptr_eq(old, &env)) {
            return;
        }
        self.instances.retain(|_, i| !matches!(i.status, PluginStatus::Missing | PluginStatus::Failed(_)));
        self.env = Some(env);
    }

    /// The plug-ins (by spec) found to edit clips over ARA since the last call, although the
    /// catalog did not say so: for the catalog to remember.
    pub fn take_ara_learned(&mut self) -> Vec<String> {
        std::mem::take(&mut self.ara_learned)
    }

    pub fn env(&self) -> Option<&PluginEnv> {
        self.env.as_deref()
    }

    /// The environment as a handle ([`std::sync::Arc`]) a worker thread holds while it renders
    /// an export off the window's.
    pub fn env_handle(&self) -> Option<std::sync::Arc<PluginEnv>> {
        self.env.clone()
    }

    /// The state of every plug-in device as it is now, by key, for saving the project. Each
    /// running plug-in is asked on a thread of its own and the answers are waited for at most
    /// [`SAVE_WAIT`], so a host process busy with something else (an editor opening over the
    /// window) cannot hold the window: one that has not answered by then gives the state it last
    /// gave, and a stopped one the state it left.
    pub(crate) fn current_states(&mut self) -> HashMap<u64, SavedState> {
        self.read_states();
        self.known_states()
    }

    /// The plug-ins' states as last read (every [`SAVE_EVERY`] and at each save), without asking
    /// them now: for a backup, which must not hold the window up while a busy plug-in answers.
    pub(crate) fn known_states(&self) -> HashMap<u64, SavedState> {
        let mut states = self.kept.clone();
        for (key, instance) in &self.instances {
            if let Some(state) = instance.control.as_ref().and_then(Plugin::last_state).or(instance.last_state.as_ref())
            {
                states.insert(*key, state.clone());
            }
        }
        states
    }

    /// Asks every running plug-in for its state, each on a thread of its own, and waits for the
    /// answers: at most [`SAVE_WAIT`] in all, with the time it takes a plug-in lent out for
    /// something else (a read just after it started) to come back first.
    fn read_states(&mut self) {
        self.read_states_until(Instant::now() + SAVE_WAIT);
    }

    /// Concurrent reads share a deadline, including control halves already lent out.
    fn read_states_until(&mut self, deadline: Instant) {
        let mut notices = Vec::new();
        let lent = |rack: &Self, keys: Option<&[u64]>| {
            rack.instances.iter().any(|(key, i)| {
                i.control.is_none() && keys.map_or(i.status == PluginStatus::Running, |keys| keys.contains(key))
            })
        };
        while lent(self, None) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
            self.take_returned(&mut notices);
        }
        let running: Vec<u64> = self.instances.iter().filter(|(_, i)| i.control.is_some()).map(|(k, _)| *k).collect();
        let asked: Vec<u64> = running.into_iter().filter(|key| self.lend(*key, read_state).is_ok()).collect();
        while lent(self, Some(&asked)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
            self.take_returned(&mut notices);
        }
        // A host that did not answer in time keeps the state it last gave; the save, render or
        // export that waited says so, so a change made in its editor may not be in it.
        let unanswered = self.instances.iter().filter(|(key, i)| asked.contains(key) && i.control.is_none()).count();
        if unanswered > 0 {
            notices.push(format!(
                "{unanswered} plug-in{} did not answer in time; the settings read are the last ones they gave",
                if unanswered == 1 { "" } else { "s" }
            ));
        }
        self.notices.extend(notices);
    }

    /// Reads every running plug-in twice before asking whether to save: processing and MIDI
    /// can change state without an editor or a host dirty notification. Both rounds share one
    /// deadline, so a busy host cannot add a second wait to leaving the project. Reads run on
    /// control workers concurrently; stopped plug-ins keep their last state.
    pub(crate) fn check_edits(&mut self) {
        while !self.poll_edits() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Advances the quit check without waiting for any host. The caller pumps frames until
    /// true; every project can make progress together under its own one-second deadline.
    pub(crate) fn poll_edits(&mut self) -> bool {
        let mut notices = Vec::new();
        self.take_returned(&mut notices);
        self.notices.extend(notices);
        let (deadline, rounds) = *self.edit_check.get_or_insert((Instant::now() + SAVE_WAIT, 2));
        if Instant::now() >= deadline {
            self.edit_check = None;
            return true;
        }
        if self.instances.values().any(|i| i.status == PluginStatus::Running && i.control.is_none()) {
            return false;
        }
        if rounds == 0 || self.instances.is_empty() {
            self.edit_check = None;
            return true;
        }
        let keys: Vec<u64> = self.instances.iter().filter(|(_, i)| i.control.is_some()).map(|(k, _)| *k).collect();
        for key in keys {
            let _busy = self.lend(key, read_state);
        }
        self.edit_check = Some((deadline, rounds - 1));
        false
    }

    /// The state reads an export gathers with, never waiting on the caller's thread: the
    /// caller runs this once per frame and its frames carry the answers, with `deadline`
    /// bounding the round the way a save's [`SAVE_WAIT`] does. `true` settles, and the states
    /// then stand where [`Rack::known_states`] reads them.
    ///
    /// The machine: while a control half is out for another reason (an editor opening, a
    /// track's ARA work), the reads wait for it, as a save's wait does; once lent, the reads
    /// wait for their answers; and past the deadline, whatever came in stands, the
    /// not-answered saying so in the notices. A frame's pumping never sleeps.
    pub(crate) fn gather_states(&mut self, deadline: Instant) -> bool {
        let mut notices = Vec::new();
        // Replies must see the active ledger. Taking the round first loses answers
        // consumed here, particularly after a tab switch removed the live instances.
        self.take_returned(&mut notices);
        // Our own reads, in flight: settled when their answers are all in — counted from the
        // round itself, not from the instances, which a project's stop takes with it.
        let mut round = self.state_gather.take();
        let mut settled = Option::<Vec<(u64, SavedState)>>::None;
        if let Some(gather) = round.as_mut() {
            // The deferred reads whose control half came back go out now, while the deadline
            // still leaves them room to be answered.
            if Instant::now() < deadline {
                let lendable: Vec<u64> = gather
                    .deferred
                    .iter()
                    .filter(|key| self.instances.get(*key).is_some_and(|i| i.control.is_some()))
                    .copied()
                    .collect();
                for key in &lendable {
                    if self.lend(*key, read_state).is_ok() {
                        gather.out.push(*key);
                    }
                }
                gather.deferred.retain(|key| !lendable.contains(key));
            }
            // The round's own count: out and not back, plus the deferred that never went.
            let pending = gather.out.iter().filter(|key| !gather.back.contains(*key)).count() + gather.deferred.len();
            if pending == 0 || Instant::now() >= deadline {
                // What came in stands; the not-answered say so by name, and their states at
                // the launch stand for them, as a save's wait does.
                for key in gather.out.iter().filter(|key| !gather.back.contains(*key)).chain(&gather.deferred) {
                    let name = gather.names.get(key).map(String::as_str).unwrap_or("a plug-in");
                    notices.push(format!("{name} did not answer in time; the settings read are the last ones it gave"));
                }
                for key in gather.back.iter().filter(|key| !gather.states.contains_key(*key)) {
                    let name = gather.names.get(key).map(String::as_str).unwrap_or("a plug-in");
                    notices.push(format!(
                        "{name}: reading its settings failed; the settings read are the last ones it gave"
                    ));
                }
                settled = Some(
                    gather
                        .out
                        .iter()
                        .chain(&gather.deferred)
                        .filter_map(|key| {
                            gather.states.get(key).or_else(|| gather.fallback.get(key)).map(|s| (*key, s.clone()))
                        })
                        .collect(),
                );
            }
        }
        if let Some(states) = settled {
            self.notices.extend(std::mem::take(&mut notices));
            for (key, state) in states {
                self.kept.insert(key, state);
            }
            return true;
        }
        if round.is_some() {
            // The round is not settled yet: its answers come on later frames.
            self.notices.extend(std::mem::take(&mut notices));
            self.state_gather = round;
            return false;
        }
        self.notices.extend(std::mem::take(&mut notices));
        // The reads go out at the first call, whatever may be holding a control half; the
        // round defers those and answers for them later. A plug-in still starting is asked
        // for the same way: its read goes out when it runs, within the round's own deadline.
        let known = self.known_states();
        let running: Vec<u64> = self
            .instances
            .iter()
            .filter(|(_, i)| matches!(i.status, PluginStatus::Running | PluginStatus::Loading))
            .map(|(k, _)| *k)
            .collect();
        if running.is_empty() {
            // Nothing running: the states stand as they are.
            return true;
        }
        let (mut lendable, deferred): (Vec<u64>, Vec<u64>) =
            running.into_iter().partition(|key| self.instances.get(key).is_some_and(|i| i.control.is_some()));
        let out: Vec<u64> = lendable.drain(..).filter(|key| self.lend(*key, read_state).is_ok()).collect();
        self.notices.extend(std::mem::take(&mut notices));
        if out.is_empty() && deferred.is_empty() {
            // No thread to ask on, and nothing that runs: the states stand as they are.
            return true;
        }
        let asked: std::collections::HashSet<u64> = out.iter().chain(&deferred).copied().collect();
        self.state_gather = Some(StateGather {
            names: asked
                .iter()
                .filter_map(|key| self.instances.get(key).and_then(|i| i.plugin.as_ref().map(|p| (*key, p.0.clone()))))
                .collect(),
            fallback: asked.iter().filter_map(|key| known.get(key).map(|s| (*key, s.clone()))).collect(),
            out,
            deferred,
            back: std::collections::HashSet::new(),
            states: HashMap::new(),
        });
        false
    }

    /// Cancels the gather's round, what came in standing where it is: a cancelled export's
    /// late replies change nothing, and the next export gathers its own round.
    pub(crate) fn cancel_gather(&mut self) {
        self.state_gather = None;
    }

    /// Whether a caller already owns the export preparation round.
    pub(crate) fn gathering(&self) -> bool {
        self.state_gather.is_some()
    }

    /// Transfers preparation notices to the export result so an inactive tab cannot hide them.
    pub(crate) fn take_export_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    /// Whether a plug-in's state changed since the project was last saved or opened, without
    /// a change to the project itself (a setting made in the plug-in's editor).
    pub(crate) fn edited(&self) -> bool {
        self.edits.changed()
    }

    /// The project was saved with `states` (from [`Rack::current_states`]).
    pub(crate) fn saved(&mut self, states: &HashMap<u64, SavedState>) {
        self.edits.saved(states);
    }

    /// Stops every plug-in, keeping each one's last state for when it starts again. The host
    /// processes are told on threads of their own, so this never waits for them.
    pub fn stop_all(&mut self) {
        let instances: Vec<(u64, Instance)> = self.instances.drain().collect();
        for (key, instance) in instances {
            if let Some(control) = instance.control {
                self.stop(key, control);
            }
        }
    }

    /// Reads `control`'s last state and lets it go, on a thread of its own; the state comes
    /// back to be kept for when the device's plug-in starts again.
    fn stop(&self, key: u64, mut control: Plugin) {
        let sender = self.returned_sender.clone();
        suhina_plugins::trace!("rack stops key {key}");
        let _detached = std::thread::Builder::new().name("suhina-plugin-stop".into()).spawn(move || {
            let state = (control.health() == Health::Running).then(|| control.save_state().ok()).flatten();
            suhina_plugins::trace!("rack stop key {key}: state {}", state.is_some());
            drop(control);
            suhina_plugins::trace!("rack stop key {key}: let go");
            let _gone = sender.send(Returned { key, generation: 0, control: None, done: Done::Stopped(state) });
        });
    }

    /// Lends the control half of plug-in device `key` to a thread of its own for `work`, so
    /// the session's thread never waits for a host process (one busy loading a sibling, or
    /// making an editor's window that waits for the session's own window). It comes back
    /// through [`Rack::take_returned`].
    fn lend(&mut self, key: u64, work: impl FnOnce(&mut Plugin) -> Done + Send + 'static) -> Result<(), CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device's plug-in is not running")?;
        let mut control = instance.control.take().ok_or("the plug-in is busy; try again in a moment")?;
        let (sender, generation) = (self.returned_sender.clone(), instance.generation);
        let spawned = std::thread::Builder::new().name("suhina-plugin-control".into()).spawn(move || {
            let done = work(&mut control);
            let _gone = sender.send(Returned { key, generation, control: Some(control), done });
        });
        spawned.map(|_| ()).map_err(|e| CommandError(format!("no thread to talk to the plug-in on: {e}")))
    }

    /// Takes back control halves lent out, and the states of stopped plug-ins.
    fn take_returned(&mut self, notices: &mut Vec<String>) {
        while let Ok(returned) = self.returned.try_recv() {
            let Returned { key, generation, control, done } = returned;
            // An export's gather keeps the reads' answers where they land, whether the
            // instances are still in or were stopped out from under the reads.
            if let Done::Saved(state) = &done
                && let Some(gather) = self.state_gather.as_mut()
                && gather.out.contains(&key)
            {
                gather.back.insert(key);
                if let Some(state) = state {
                    gather.states.insert(key, state.clone());
                }
            }
            if let Done::Stopped(state) = done {
                // A plug-in loaded again with new clips starts with this state.
                self.rebinding.remove(&key);
                if let Some(state) = state {
                    self.edits.read(key, &state);
                    self.kept.insert(key, state);
                }
                continue;
            }
            // A clip's edits on their way: stored, or given to the other track's plug-in.
            match &done {
                Done::AraExported(number, result) => self.exported(*number, result.clone(), notices),
                Done::AraImported(number, result) => self.imported(*number, result.clone(), notices),
                _ => {}
            }
            let Some(control) = control else { continue };
            let mut reopen = false;
            let mut fall_back = false;
            match self.instances.get_mut(&key).filter(|i| i.generation == generation && i.control.is_none()) {
                Some(instance) => {
                    let name = instance.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
                    match done {
                        Done::Opened(Ok(size)) => {
                            instance.editor_open = true;
                            instance.editor_size = Some(size);
                        }
                        Done::Opened(Err(why)) => {
                            // Some systems cannot show an editor inside this window's process
                            // (macOS): a window of its own still can. One fall back to it, so
                            // the default setting works everywhere.
                            if instance.editor_embedded && !instance.force_floating {
                                instance.force_floating = true;
                                instance.editor_gone();
                                fall_back = true;
                            } else {
                                notices.push(format!("{name}: its editor did not open ({why})"));
                            }
                        }
                        Done::Closed(result) => {
                            instance.editor_gone();
                            reopen = std::mem::take(&mut instance.reopen_editor);
                            if let Err(why) = result {
                                notices.push(format!("{name}: its editor did not close ({why})"));
                            }
                        }
                        Done::AraUpdated(Ok(())) | Done::AraExported(..) | Done::AraImported(..) => {}
                        Done::AraUpdated(Err(why)) => {
                            notices.push(format!(
                                "{name}: its clips could not be updated ({why}); it loads again with them"
                            ));
                            instance.ara_failed = true;
                        }
                        Done::Listed(list) => instance.list_done(list, Instant::now()),
                        Done::Saved(Some(state)) => {
                            self.edits.read(key, &state);
                            instance.last_state = Some(state);
                        }
                        Done::Saved(None) | Done::Stopped(_) => {}
                    }
                    instance.control = Some(control);
                }
                // The device went while its plug-in was out: stop it like the others.
                None => self.stop(key, control),
            }
            if (reopen || fall_back)
                && let Err(e) = self.open_editor(key)
            {
                notices.push(format!("the editor did not open again: {}", e.0));
            }
            // An open or close asked meanwhile, now that the control half is back.
            let wanted = self.instances.get_mut(&key).and_then(|i| i.editor_wanted.take());
            let result = match wanted {
                Some(true) if !self.instances.get(&key).is_some_and(|i| i.editor_open) => {
                    self.open_editor(key).map(|_| ())
                }
                Some(false) if self.instances.get(&key).is_some_and(|i| i.editor_open) => self.close_editor(key),
                _ => Ok(()),
            };
            if let Err(e) = result {
                notices.push(e.0);
            }
        }
    }

    /// Tells the hosts where the window wants their embedded editors, where that has not been
    /// told yet (the control half was out when the window said).
    fn send_placements(&mut self) {
        for (key, instance) in &mut self.instances {
            let wanted = self.ara_selection.get(key).cloned().unwrap_or_default();
            if instance.ara_sources.is_empty() || instance.ara_selection_sent.as_ref() == Some(&wanted) {
                continue;
            }
            if let Some(control) = instance.control.as_mut()
                && control.ara_select(wanted.as_deref()).unwrap_or(false)
            {
                instance.ara_selection_sent = Some(wanted);
            }
        }
        for instance in self.instances.values_mut() {
            if let Some(fit) = instance.fit_wanted
                && instance.fit_sent != Some(fit)
                && instance.send_fit(fit)
            {
                instance.fit_sent = Some(fit);
            }
        }
        for instance in self.instances.values_mut() {
            let Some(wanted) = instance.placement_wanted else { continue };
            if instance.placement_sent != Some(wanted) && instance.send_placement(wanted) {
                instance.placement_sent = Some(wanted);
            }
        }
    }

    /// Lists the parameters of the plug-in device `key` again, in the background, for its
    /// generated panel, when it runs and its list is older than [`LIST_EVERY`]; the next
    /// [`Rack::report`] after it answers carries it. The window asks for this while it shows the
    /// device's panel; commands never do, so they do not find the plug-in busy.
    pub fn list_params(&mut self, key: u64) {
        let stale = self.instances.get(&key).is_some_and(|i| i.control.is_some() && i.list_due(Instant::now()));
        if stale
            && self.lend(key, |control| Done::Listed(control.params_within(LIST_TIMEOUT).ok())).is_ok()
            && let Some(instance) = self.instances.get_mut(&key)
        {
            instance.listing = true;
        }
    }

    /// What is known about the plug-in device `key`.
    pub fn report(&mut self, key: u64) -> Option<PluginReport> {
        let instance = self.instances.get_mut(&key)?;
        if let Some(control) = instance.control.as_mut() {
            if control.editor_open() {
                instance.editor_open = true;
                instance.editor_size = control.editor_size();
            } else {
                instance.editor_gone();
            }
        }
        Some(PluginReport {
            status: instance.status.clone(),
            plugin: instance.plugin.clone(),
            instrument: instance.info.instrument,
            latency: instance.info.latency,
            editor_open: instance.editor_open,
            editor_embedded: instance.editor_size.filter(|_| instance.editor_embedded && instance.editor_open),
            blocks: instance.counts.as_ref().map_or(0, |c| c.read().0),
            late_blocks: instance.counts.as_ref().map_or(0, |c| c.read().1),
            longest_us: instance.counts.as_ref().map_or(0, |c| c.longest_us()),
            no_time: instance.counts.as_ref().map_or(0, |c| c.no_time()),
            state_refused: instance.state_refused.clone(),
            ara: !instance.ara_sources.is_empty(),
            params: instance
                .listed
                .as_ref()
                .filter(|_| instance.status == PluginStatus::Running)
                .map(|(list, _)| Arc::clone(list)),
        })
    }

    /// How far the plug-ins have got with starting.
    pub fn progress(&self) -> PluginProgress {
        let mut progress = PluginProgress::default();
        let name = |i: &Instance| i.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
        for instance in self.instances.values() {
            match &instance.status {
                PluginStatus::Missing => progress.missing += 1,
                PluginStatus::Running => progress.ready += 1,
                PluginStatus::Loading => progress.loading.push(name(instance)),
                PluginStatus::Crashed(_) | PluginStatus::Failed(_) => progress.failed.push(name(instance)),
            }
        }
        progress.total = self.instances.len() - progress.missing;
        progress.loading.sort();
        progress.failed.sort();
        progress
    }

    /// The slots the engine's graphs are built with.
    fn slot_info(&self) -> HashMap<u64, PluginSlotInfo> {
        self.instances
            .iter()
            .filter(|(_, i)| i.status != PluginStatus::Missing)
            .map(|(key, i)| (*key, i.info))
            .collect()
    }

    /// Brings the running plug-ins in line with `project`: starts the ones new devices need
    /// (in the background), stops the ones whose devices are gone, takes in the ones that have
    /// started and looks after their health. Returns what the user should hear about.
    pub fn sync(&mut self, project: &Project, audio: &AudioPool, player: Option<&mut Player>) -> Vec<String> {
        let mut notices = std::mem::take(&mut self.notices);
        let Some(player) = player else {
            self.stop_all();
            return notices;
        };
        let Some(env) = self.env.clone() else { return notices };
        let sample_rate = player.sample_rate();
        if sample_rate != self.sample_rate {
            self.stop_all();
            self.sample_rate = sample_rate;
        }
        let devices = plugin_devices(project);
        let mut changed = false;
        // Devices that are gone.
        let gone: Vec<u64> = self.instances.keys().copied().filter(|k| !devices.iter().any(|d| d.key == *k)).collect();
        for key in gone {
            if let Some(instance) = self.instances.remove(&key) {
                if let Some(control) = instance.control {
                    self.stop(key, control);
                }
                changed = true;
            }
        }
        changed |= self.follow_clips(project, audio, &devices);
        self.move_carries(&devices, &mut notices);
        // New ones.
        let mut missing = Vec::new();
        for found in &devices {
            if self.instances.contains_key(&found.key) || self.rebinding.contains(&found.key) {
                continue;
            }
            let Some(device) = project
                .channels
                .iter()
                .find(|c| c.id == found.channel)
                .and_then(|c| suhina_model::device_at(&c.devices, &found.path))
            else {
                continue;
            };
            changed = true;
            let Some((matched, instrument)) = find(&env.catalog, device) else {
                missing.push(device.name.clone());
                self.instances.insert(found.key, Instance::new(PluginStatus::Missing, None, 0));
                continue;
            };
            let state = self.kept.remove(&found.key).or_else(|| saved_state(device));
            // A plug-in that edits clips gets its track's audio clips (ADR 0028).
            let spec = matched.plugin.spec();
            let edits_clips =
                matched.plugin.ara || self.ara_found.contains(&found.key) || self.ara_plugins.contains(&spec);
            let layout = found.track.filter(|_| edits_clips).map(|t| crate::ara::Layout::of(project, audio, t));
            let ara = layout.as_ref().and_then(|l| l.document(audio)).map(|(mut document, sources)| {
                document.document.clone_from(&self.ara_document);
                (document, sources)
            });
            let (ara, ara_sources) = ara.map_or((None, Vec::new()), |(document, sources)| (Some(document), sources));
            let launch = Launch {
                host: env.host.clone(),
                plugin: matched.plugin.spec(),
                channels: 2,
                max_frames: MAX_BLOCK_FRAMES as u32,
                sample_rate: f64::from(sample_rate),
                timeout: START_TIMEOUT,
                group: (env.group)(matched.plugin),
                ara,
            };
            self.next_generation += 1;
            let generation = self.next_generation;
            let plugin = Some((matched.plugin.name.clone(), matched.plugin.vendor.clone(), matched.how.name()));
            let mut instance = Instance::new(PluginStatus::Loading, plugin, generation);
            instance.spec = spec;
            instance.track = found.track.filter(|_| edits_clips);
            instance.info = PluginSlotInfo { instrument, latency: 0 };
            instance.ara_layout = layout;
            instance.ara_sources = ara_sources;
            self.instances.insert(found.key, instance);
            spawn_start(self.sender.clone(), found.key, generation, launch, state);
        }
        if !missing.is_empty() {
            notices.push(missing_notice(&env, &missing));
        }
        changed |= self.receive(player, &mut notices);
        self.take_returned(&mut notices);
        changed |= self.follow_latencies();
        self.follow_notes(project, &devices);
        self.send_placements();
        self.watch(&mut notices);
        if changed {
            player.set_plugin_info(self.slot_info());
            if let Err(e) = player.update(project) {
                notices.push(format!("plug-ins: {}", e.0));
            }
        }
        notices
    }

    /// Carries a clip's edits from the plug-in `from_key` to the one that edits clips on
    /// `to_track` (ADR 0028 step 6): its modification `from` there becomes `to` here, of
    /// `source`; it arrives as region `region`. Done over the next syncs; a notice when it
    /// cannot be.
    pub(crate) fn carry(
        &mut self,
        from_key: u64,
        to_track: suhina_model::TrackId,
        (from, to): (String, String),
        source: String,
        region: String,
    ) {
        self.next_carry += 1;
        self.carries.push(Carry {
            number: self.next_carry,
            from_key,
            to_track,
            from,
            to,
            source,
            region,
            archive: None,
            busy: false,
            since: Instant::now(),
        });
    }

    /// The clips' edits on their way: whether any is still waiting.
    pub(crate) fn carrying(&self) -> bool {
        !self.carries.is_empty()
    }

    /// Takes each carried clip's edits a step on: stored from its first plug-in when that one is
    /// free, given to the other track's plug-in once it runs with the clip there.
    fn move_carries(&mut self, devices: &[suhina_engine::PluginDevice], notices: &mut Vec<String>) {
        let mut gone = Vec::new();
        let mut lend: Vec<(u64, u64, bool)> = Vec::new();
        for carry in &mut self.carries {
            if carry.since.elapsed() >= CARRY_FOR {
                gone.push(carry.number);
                notices
                    .push(format!("a clip's edits did not reach its new track's plug-in in {} s", CARRY_FOR.as_secs()));
                continue;
            }
            if carry.busy {
                continue;
            }
            match &carry.archive {
                None => {
                    let ready = self.instances.get(&carry.from_key).is_some_and(|i| i.control.is_some());
                    if ready {
                        lend.push((carry.number, carry.from_key, true));
                    }
                }
                Some(_) => {
                    // The plug-in that edits clips on the track, running with the clip there.
                    let target = devices.iter().filter(|d| d.track == Some(carry.to_track)).find(|d| {
                        self.instances.get(&d.key).is_some_and(|i| {
                            i.status == PluginStatus::Running
                                && i.control.is_some()
                                && i.ara_layout.as_ref().is_some_and(|l| l.has_region(&carry.region))
                        })
                    });
                    if let Some(target) = target {
                        lend.push((carry.number, target.key, false));
                    }
                }
            }
        }
        self.carries.retain(|c| !gone.contains(&c.number));
        for (number, key, export) in lend {
            let Some(carry) = self.carries.iter_mut().find(|c| c.number == number) else { continue };
            let lent = if export {
                let (modification, source) = (carry.from.clone(), carry.source.clone());
                carry.busy = true;
                self.lend(key, move |control| {
                    Done::AraExported(number, control.ara_export(&modification, &source).map_err(|e| e.to_string()))
                })
            } else {
                let Some((id, bytes)) = carry.archive.clone() else { continue };
                let (from, to, source) = (carry.from.clone(), carry.to.clone(), carry.source.clone());
                carry.busy = true;
                self.lend(key, move |control| {
                    let imported = control.ara_import((&id, &bytes), &from, &to, &source);
                    Done::AraImported(number, imported.map_err(|e| e.to_string()))
                })
            };
            if lent.is_err()
                && let Some(carry) = self.carries.iter_mut().find(|c| c.number == number)
            {
                carry.busy = false;
            }
        }
    }

    fn exported(&mut self, number: u64, result: Result<(String, Vec<u8>), String>, notices: &mut Vec<String>) {
        let Some(at) = self.carries.iter().position(|c| c.number == number) else { return };
        match result {
            Ok(archive) => {
                suhina_plugins::trace!("rack carries edits {number}: stored, {} bytes", archive.1.len());
                self.carries[at].archive = Some(archive);
                self.carries[at].busy = false;
            }
            Err(why) => {
                self.carries.remove(at);
                notices.push(format!("a clip's edits could not be taken along ({why})"));
            }
        }
    }

    fn imported(&mut self, number: u64, result: Result<(), String>, notices: &mut Vec<String>) {
        let Some(at) = self.carries.iter().position(|c| c.number == number) else { return };
        match result {
            Ok(()) => {
                suhina_plugins::trace!("rack carries edits {number}: given to the new track's plug-in");
                self.carries.remove(at);
            }
            // The clip has not reached that document yet: asked again at a later sync.
            Err(why) if why.contains("yet") => self.carries[at].busy = false,
            Err(why) => {
                self.carries.remove(at);
                notices.push(format!("a clip's edits could not be given to its new track's plug-in ({why})"));
            }
        }
    }

    /// Notices plug-ins that edit clips whose track's clips changed (moved, cut, added or
    /// removed): once the clips have stayed put a moment, each is stopped and, when its state
    /// with its edits is back, loaded again with the new clips; the edits of clips that are
    /// still there come back with it (ADR 0028). Returns whether one was stopped.
    fn follow_clips(&mut self, project: &Project, audio: &AudioPool, devices: &[suhina_engine::PluginDevice]) -> bool {
        let mut rebind = Vec::new();
        let mut updates = Vec::new();
        for found in devices {
            let Some(instance) = self.instances.get_mut(&found.key) else { continue };
            // Found to support ARA when it loaded: on a track with audio clips it loads again
            // with them.
            if instance.ara_layout.is_none()
                && self.ara_found.contains(&found.key)
                && instance.status == PluginStatus::Running
                && instance.control.is_some()
                && let Some(track) = found.track
            {
                let now = crate::ara::Layout::of(project, audio, track);
                if now.is_empty() {
                    instance.ara_layout = Some(now);
                } else {
                    rebind.push(found.key);
                }
                continue;
            }
            let (Some(layout), Some(track)) = (&instance.ara_layout, found.track) else { continue };
            if instance.status != PluginStatus::Running || instance.control.is_none() {
                continue;
            }
            if instance.ara_failed {
                rebind.push(found.key);
                continue;
            }
            let now = crate::ara::Layout::of(project, audio, track);
            if now == *layout {
                instance.ara_changed = None;
                continue;
            }
            // Bound to a document: the change goes to it at once, as an edit (ADR 0028
            // decision 3). Moved and trimmed clips change in place; nothing reloads.
            if !instance.ara_sources.is_empty()
                && let Some(mut document) = now.document_with(audio, &mut instance.ara_sources)
            {
                document.document.clone_from(&self.ara_document);
                // A clip that got its own content starts with the edits of the one it copies.
                for (region, origin) in document.regions.iter_mut().zip(now.clone_origins(layout)) {
                    region.clone_of = origin;
                }
                instance.ara_layout = Some(now);
                // The regions may be new: the editor is told the selection again.
                instance.ara_selection_sent = None;
                updates.push((found.key, document));
                continue;
            }
            let since = *instance.ara_changed.get_or_insert_with(Instant::now);
            if since.elapsed() >= REBIND_AFTER {
                rebind.push(found.key);
            }
        }
        for (key, document) in updates {
            let lent = self
                .lend(key, move |control| Done::AraUpdated(control.ara_update(document).map_err(|e| e.to_string())));
            if lent.is_err()
                && let Some(instance) = self.instances.get_mut(&key)
            {
                instance.ara_failed = true;
            }
        }
        for key in &rebind {
            suhina_plugins::trace!("rack rebinds key {key}");
            let Some(instance) = self.instances.remove(key) else { continue };
            if instance.editor_open {
                self.reopen_after_rebind.insert(*key);
            }
            if let Some(control) = instance.control {
                self.rebinding.insert(*key);
                self.stop(*key, control);
            }
        }
        !rebind.is_empty()
    }

    /// Takes in plug-ins that have started: hands each one's audio half to the engine. Returns
    /// whether what the graphs need to know changed (a latency).
    fn receive(&mut self, player: &mut Player, notices: &mut Vec<String>) -> bool {
        let mut changed = false;
        let mut reopen = Vec::new();
        while let Ok(started) = self.receiver.try_recv() {
            let Some(instance) = self.instances.get_mut(&started.key) else { continue };
            if instance.generation != started.generation {
                continue;
            }
            if let Some(state) = started.first_state {
                self.edits.read(started.key, &state);
                instance.last_state = Some(state);
            }
            let name = instance.plugin.as_ref().map_or_else(String::new, |p| p.0.clone());
            match started.result {
                Ok((control, audio, refused)) => {
                    if let Some(why) = &refused {
                        notices.push(format!("{name} did not take its saved settings ({why}); it runs with its own"));
                    }
                    let latency = control.latency();
                    if latency != instance.info.latency {
                        instance.info.latency = latency;
                        changed = true;
                    }
                    instance.state_refused = refused;
                    // A plug-in with an ARA document plays its regions at the song's position.
                    let hosted = Hosted::new(audio);
                    instance.counts = Some(hosted.counts());
                    match player.send_plugin(started.key, Box::new(hosted)) {
                        Ok(()) => instance.status = PluginStatus::Running,
                        Err(e) => instance.status = PluginStatus::Failed(e.0),
                    }
                    // The host found it supports ARA where the catalog did not say so: it is
                    // loaded again with its track's clips ([`Rack::follow_clips`]).
                    if control.edits_clips() && instance.ara_layout.is_none() && self.ara_found.insert(started.key) {
                        if !instance.spec.is_empty() && self.ara_plugins.insert(instance.spec.clone()) {
                            self.ara_learned.push(instance.spec.clone());
                        }
                        notices
                            .push(format!("{name} edits clips (ARA): it is loaded again with its track's audio clips"));
                    }
                    // A restarted plug-in has a new instance number: placements go to it, again.
                    instance.placer = None;
                    instance.placement_sent = None;
                    instance.fit_sent = None;
                    instance.control = Some(control);
                    // Loaded again with its track's new clips: its editor was open, so it opens
                    // again where it was.
                    let wanted = instance.editor_wanted.take() == Some(true);
                    if (self.reopen_after_rebind.remove(&started.key) || wanted)
                        && instance.status == PluginStatus::Running
                    {
                        reopen.push(started.key);
                    }
                }
                Err(why) => {
                    notices.push(format!("{name} could not start: {why}"));
                    instance.status = PluginStatus::Failed(why);
                }
            }
        }
        for key in reopen {
            if let Err(e) = self.open_editor(key) {
                notices.push(format!("the editor did not open again: {}", e.0));
            }
        }
        changed
    }

    /// Takes in the latencies running plug-ins reported since they started (a lookahead the user
    /// changed, say). Returns whether what the graphs need to know changed: a new graph then
    /// lines the tracks up again.
    fn follow_latencies(&mut self) -> bool {
        let mut changed = false;
        for instance in self.instances.values_mut() {
            if instance.status != PluginStatus::Running {
                continue;
            }
            let Some(control) = instance.control.as_mut() else { continue };
            let latency = control.follow_latency();
            if latency != instance.info.latency {
                instance.info.latency = latency;
                changed = true;
            }
        }
        changed
    }

    /// Takes in the notes ARA plug-ins found in their clips, and forgets those of clips no
    /// running, enabled plug-in edits any more (moved off its track, deleted, the device bypassed
    /// or removed).
    fn follow_notes(&mut self, project: &Project, devices: &[suhina_engine::PluginDevice]) {
        let enabled = |key: u64| {
            devices.iter().find(|d| d.key == key).is_some_and(|found| {
                project
                    .channels
                    .iter()
                    .find(|c| c.id == found.channel)
                    .and_then(|c| suhina_model::device_at(&c.devices, &found.path))
                    .is_some_and(|d| d.enabled)
            })
        };
        // Taken in from a bypassed plug-in too, only not shown: the host sends notes when they
        // change, so they are there when it is switched on again.
        let (mut live, mut shown): (Vec<&crate::ara::Layout>, Vec<&crate::ara::Layout>) = (Vec::new(), Vec::new());
        for (key, instance) in &mut self.instances {
            if instance.status != PluginStatus::Running {
                continue;
            }
            let Some(layout) = &instance.ara_layout else { continue };
            // The notes it has are kept while its control half is lent out (an edit of its clips
            // going to it, an editor opening): the host sends notes only when they change, so
            // ones let go of then would not come back. New ones are taken in when it is home.
            live.push(layout);
            if enabled(*key) {
                shown.push(layout);
            }
            let Some(control) = instance.control.as_mut() else { continue };
            if let Some(progress) = control.take_ara_analysis() {
                instance.ara_progress = progress;
            }
            for (region, notes) in control.take_ara_notes() {
                let Some(clip) = region.strip_prefix("clip-").and_then(|n| n.parse().ok()).map(suhina_model::ClipId)
                else {
                    continue;
                };
                if !layout.has_region(&region) {
                    continue;
                }
                suhina_plugins::trace!("app gets ara notes for {region}: {}", notes.len());
                match crate::ara::ClipNotes::new(notes) {
                    Some(notes) => self.ara_notes.insert(clip, notes),
                    None => self.ara_notes.remove(&clip),
                };
            }
        }
        let has = |layouts: &[&crate::ara::Layout], clip: &suhina_model::ClipId| {
            let region = format!("clip-{}", clip.0);
            layouts.iter().any(|l| l.has_region(&region))
        };
        self.ara_notes.retain(|clip, _| has(&live, clip));
        self.ara_notes_hidden = self.ara_notes.keys().filter(|clip| !has(&shown, clip)).copied().collect();
    }

    /// What an ARA plug-in on `track` is doing with its audio clips that they should show: its
    /// name and how far its analysis has come (`None` while it loads).
    pub(crate) fn ara_activity(&self, track: suhina_model::TrackId) -> Option<(String, Option<f32>)> {
        self.instances.values().filter(|i| i.track == Some(track)).find_map(|i| {
            let name = i.plugin.as_ref().map_or_else(|| "The plug-in".to_string(), |p| p.0.clone());
            match &i.status {
                PluginStatus::Loading => Some((name, None)),
                PluginStatus::Running => i.ara_progress.map(|p| (name, Some(p))),
                _ => None,
            }
        })
    }

    /// The notes an ARA plug-in found in `clip`, if any (#720).
    pub(crate) fn clip_notes(&self, clip: suhina_model::ClipId) -> Option<&crate::ara::ClipNotes> {
        if self.ara_notes_hidden.contains(&clip) {
            return None;
        }
        self.ara_notes.get(&clip)
    }

    /// Notices plug-ins that crashed or hung, and now and then reads each one's state.
    fn watch(&mut self, notices: &mut Vec<String>) {
        if self.checked.elapsed() < HEALTH_EVERY {
            return;
        }
        self.checked = Instant::now();
        let save = self.saved.elapsed() >= self.save_every;
        if save {
            self.saved = Instant::now();
        }
        let mut to_save = Vec::new();
        for (key, instance) in self.instances.iter_mut() {
            let Some(control) = instance.control.as_mut() else { continue };
            if instance.status != PluginStatus::Running {
                continue;
            }
            let name = instance.plugin.as_ref().map_or("A plug-in", |p| p.0.as_str());
            match control.health() {
                Health::Running => {
                    // Processing and MIDI can change state even without an editor, and not
                    // every format/plug-in sends dirty notifications. Refresh all states for
                    // backups. Defer an open ARA editor's expensive archive until it closes.
                    let editing_ara = instance.editor_open && instance.ara_layout.is_some();
                    if save && !editing_ara {
                        to_save.push(*key);
                    }
                }
                Health::Crashed => {
                    notices
                        .push(format!("{name} crashed; the project plays on without it (restart it from its device)"));
                    instance.status = PluginStatus::Crashed("its host process exited".into());
                }
                Health::Hung => {
                    notices.push(format!(
                        "{name} stopped answering; the project plays on without it (restart it from its device)"
                    ));
                    instance.status = PluginStatus::Crashed("it stopped answering".into());
                }
            }
        }
        // Kept by the control half for a restart; read where waiting does not matter.
        for key in to_save {
            let _busy_then_next_time = self.lend(key, read_state);
        }
    }

    /// Restarts the plug-in device `key` in a fresh host process with its last state, in the
    /// background; it plays again once it has started.
    pub fn restart(&mut self, key: u64) -> Result<(), CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device has no plug-in running")?;
        if instance.status == PluginStatus::Missing {
            return Err("the plug-in is not installed; there is nothing to restart".into());
        }
        let Some(control) = instance.control.take() else {
            return match instance.status {
                // It never started: start it again from scratch, as when the project opened.
                PluginStatus::Failed(_) | PluginStatus::Crashed(_) => {
                    self.instances.remove(&key);
                    Ok(())
                }
                PluginStatus::Loading => Err("the plug-in is still starting".into()),
                _ => Err("the plug-in is busy; try again in a moment".into()),
            };
        };
        self.next_generation += 1;
        instance.generation = self.next_generation;
        instance.status = PluginStatus::Loading;
        // The new process may list other parameters: its panel waits for its own list.
        instance.listed = None;
        instance.list_tried = None;
        let (sender, generation) = (self.sender.clone(), self.next_generation);
        let spawned = std::thread::Builder::new().name("suhina-plugin-restart".into()).spawn(move || {
            let mut control = control;
            let result = control.restart().map(|audio| (control, audio, None)).map_err(|e| e.to_string());
            let _gone = sender.send(Started { key, generation, result, first_state: None });
        });
        spawned.map(|_| ()).map_err(|e| CommandError(format!("could not restart the plug-in: {e}")))
    }

    /// How often each running plug-in's state is read from now on (every [`SAVE_EVERY`] unless
    /// set), so a test of those reads need not wait half a minute.
    pub fn set_save_every(&mut self, every: Duration) {
        self.save_every = every;
    }

    /// The window editors open over from now on: a Win32 `HWND` or an X11 window id.
    pub fn set_owner(&mut self, owner: Option<u64>) {
        self.owner = owner;
    }

    /// Whether editors open embedded in the owner window's device panel from now on, rather
    /// than in windows of their own (nothing without an owner window). An editor open the
    /// other way is closed and opened again the new way.
    pub fn set_embedding(&mut self, embed: bool) {
        let embed = embed && self.owner.is_some();
        if self.embed == embed {
            return;
        }
        self.embed = embed;
        let to_move: Vec<u64> = self
            .instances
            .iter()
            // An ARA plug-in's editor goes its own way ([`Rack::set_ara_floating`]).
            .filter(|(_, i)| i.ara_sources.is_empty())
            .filter(|(_, i)| (i.editor_open && i.editor_embedded != embed) || (!embed && i.force_floating))
            .map(|(key, _)| *key)
            .collect();
        for key in to_move {
            if let Some(instance) = self.instances.get_mut(&key) {
                instance.force_floating = false;
            }
            if self.close_editor(key).is_ok()
                && let Some(instance) = self.instances.get_mut(&key)
            {
                instance.reopen_editor = true;
            }
        }
    }

    /// Takes the editor of the plug-in device `key`, one that edits clips, out of the window to
    /// float in one of its own (`float`), or back in. An open editor closes and opens again the
    /// new way; a closed one opens that way next time.
    pub fn set_ara_floating(&mut self, key: u64, float: bool) -> Result<(), CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device's plug-in is not running")?;
        instance.ara_float = float;
        if !float {
            instance.force_floating = false;
        }
        if instance.editor_open && instance.editor_embedded == float {
            self.close_editor(key)?;
            if let Some(instance) = self.instances.get_mut(&key) {
                instance.reopen_editor = true;
            }
        }
        Ok(())
    }

    /// The parameters of the plug-in device `key` as its plug-in lists them now (it waits for
    /// the host's answer), remembered so values set later can be kept in range.
    pub fn params(&mut self, key: u64) -> Result<Vec<suhina_plugins::ParamInfo>, CommandError> {
        // Out for its panel's listing: the list it gave last is as good, and nothing waits.
        if let Some(instance) = self.instances.get(&key)
            && instance.listing
            && let Some((list, _)) = &instance.listed
        {
            return Ok(list.to_vec());
        }
        let list = self.running(key)?.params().map_err(|e| CommandError(e.to_string()))?;
        if let Some(instance) = self.instances.get_mut(&key) {
            instance.ranges = Some(ranges(&list));
        }
        Ok(list)
    }

    /// The range of parameter `id` of the plug-in device `key`: `None` when its parameters
    /// have not been listed, `Some(None)` when they have and it has no such parameter.
    pub fn param_range(&self, key: u64, id: u32) -> Option<Option<(f64, f64)>> {
        let ranges = self.instances.get(&key)?.ranges.as_ref()?;
        Some(ranges.iter().find(|(i, _, _)| *i == id).map(|&(_, min, max)| (min, max)))
    }

    /// Opens the editor of the plug-in device `key`; returns the size of its inside. Over an
    /// owner window (the desktop app's) it opens on a thread of its own and `None` comes back
    /// at once: the host making a window that the app's window owns waits for the app's window
    /// thread, which must not be waiting for the host meanwhile. Embedded (the setting), it
    /// opens hidden inside that window until [`Rack::place_editors`] puts it somewhere.
    pub fn open_editor(&mut self, key: u64) -> Result<Option<(u32, u32)>, CommandError> {
        if self.lent_out(key, true) {
            return Ok(None);
        }
        self.running(key)?;
        match self.owner {
            Some(owner) => {
                // A plug-in that edits clips opens in the clip's place in the window, as a host
                // with a built-in pitch editor does, unless it was taken out to float.
                let embed = self.instances.get(&key).is_some_and(|i| {
                    let wanted = if i.ara_sources.is_empty() { self.embed } else { !i.ara_float };
                    wanted && !i.force_floating
                });
                self.lend(key, move |control| {
                    let opened = if embed {
                        control.open_editor_embedded(owner)
                    } else {
                        control.open_editor_owned(Some(owner))
                    };
                    Done::Opened(opened.map_err(|e| e.to_string()))
                })?;
                if let Some(instance) = self.instances.get_mut(&key) {
                    instance.editor_embedded = embed;
                    instance.placement_sent = None;
                }
                Ok(None)
            }
            None => self.running(key)?.open_editor().map(Some).map_err(|e| CommandError(e.to_string())),
        }
    }

    /// Whether the plug-in device `key` runs bound to an ARA document of its track's clips.
    pub fn edits_clips(&self, key: u64) -> bool {
        self.instances.get(&key).is_some_and(|i| i.status == PluginStatus::Running && !i.ara_sources.is_empty())
    }

    /// The room in pixels the window has for the embedded editors of these plug-in devices,
    /// which are asked to fit it (a plug-in that lets its editor be resized takes it, within
    /// its limits). Told to the hosts now where it can be, else at the next [`Rack::sync`].
    pub fn fit_editors(&mut self, wanted: &[(u64, (u32, u32))]) {
        for (key, size) in wanted {
            if let Some(instance) = self.instances.get_mut(key)
                && instance.editor_open
                && instance.editor_embedded
            {
                instance.fit_wanted = Some(*size);
            }
        }
        self.send_placements();
    }

    /// Which region (`clip-<id>`) the plug-in device `key`, one that edits clips, should show
    /// as selected in its editor; `None` selects nothing. Told to it now where it can be, else
    /// at the next [`Rack::sync`], and again whenever it is loaded again. Never waits.
    pub fn select_ara(&mut self, key: u64, region: Option<String>) {
        if self.ara_selection.get(&key) == Some(&region) {
            return;
        }
        self.ara_selection.insert(key, region);
        self.send_placements();
    }

    /// Where the window shows the embedded editors this frame, by plug-in device key; every
    /// other embedded editor is hidden (its device is not in the panel). Told to the hosts now
    /// where it can be (and the place changed), otherwise at the next [`Rack::sync`]. Never
    /// waits for a host. Nothing for editors in windows of their own.
    pub fn place_editors(&mut self, shown: &[(u64, Placement)]) {
        for (key, instance) in &mut self.instances {
            if !instance.editor_embedded || !instance.editor_open {
                continue;
            }
            let placement = shown.iter().find(|(k, _)| k == key).map_or(Placement::HIDDEN, |(_, p)| *p);
            instance.placement_wanted = Some(placement);
            if instance.placement_sent == Some(placement) {
                continue;
            }
            if instance.send_placement(placement) {
                instance.placement_sent = Some(placement);
            }
        }
    }

    /// Closes the editor of the plug-in device `key`; with an owner window, on a thread of
    /// its own, like opening it.
    pub fn close_editor(&mut self, key: u64) -> Result<(), CommandError> {
        if self.lent_out(key, false) {
            return Ok(());
        }
        self.running(key)?;
        if self.owner.is_some() {
            return self.lend(key, |control| Done::Closed(control.close_editor().map_err(|e| e.to_string())));
        }
        self.running(key)?.close_editor().map_err(|e| CommandError(e.to_string()))
    }

    /// Whether plug-in `key` cannot take the editor's opening or closing now: its control half
    /// is out on a thread of its own, it is still starting, or it is being loaded again with its
    /// track's clips. The editor `open` (or closed) is then remembered and done when it can be
    /// ([`Rack::take_returned`], [`Rack::receive`]), so a click then never says it is busy or
    /// still starting.
    fn lent_out(&mut self, key: u64, open: bool) -> bool {
        // Being loaded again with its track's clips: its editor opens (or stays shut) once it is.
        if self.rebinding.contains(&key) && !self.instances.contains_key(&key) {
            if open {
                self.reopen_after_rebind.insert(key);
            } else {
                self.reopen_after_rebind.remove(&key);
            }
            return true;
        }
        let Some(instance) = self.instances.get_mut(&key) else { return false };
        // Still starting, or busy on another thread: the editor follows when it is free.
        let out = instance.status == PluginStatus::Loading
            || (instance.status == PluginStatus::Running && instance.control.is_none());
        if out {
            instance.editor_wanted = Some(open);
        }
        out
    }

    fn running(&mut self, key: u64) -> Result<&mut Plugin, CommandError> {
        let instance = self.instances.get_mut(&key).ok_or("the device's plug-in is not running")?;
        match &instance.status {
            PluginStatus::Running => {
                instance.control.as_mut().ok_or_else(|| "the plug-in is busy; try again in a moment".into())
            }
            PluginStatus::Loading => Err("the plug-in is still starting".into()),
            PluginStatus::Missing => Err("the plug-in is not installed".into()),
            PluginStatus::Crashed(why) => Err(format!("the plug-in crashed ({why}); restart it first").into()),
            PluginStatus::Failed(why) => Err(format!("the plug-in could not start: {why}").into()),
        }
    }

    /// Waits until no plug-in is starting any more, taking each in as it comes (tests, and
    /// callers that need every plug-in before going on), at most `timeout`.
    pub fn wait_started(
        &mut self,
        project: &Project,
        audio: &AudioPool,
        player: &mut Player,
        timeout: Duration,
    ) -> Vec<String> {
        let deadline = Instant::now() + timeout;
        let mut notices = self.sync(project, audio, Some(player));
        while self.instances.values().any(|i| i.status == PluginStatus::Loading) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            notices.extend(self.sync(project, audio, Some(player)));
        }
        notices
    }
}

/// What the status bar says about plug-ins that were not found: why, when the catalog is empty
/// (no scan yet, or the host to scan with is missing), else which.
fn missing_notice(env: &PluginEnv, missing: &[String]) -> String {
    if env.catalog.plugins().next().is_none() {
        if !env.host.exists() {
            return format!(
                "{} plug-ins cannot be looked for: the plug-in host {} is missing",
                missing.len(),
                env.host.display()
            );
        }
        return format!(
            "{} plug-ins cannot be found yet: no plug-in scan has run (Settings › Plug-ins › Scan)",
            missing.len()
        );
    }
    format!(
        "{} plug-in{} not installed, kept with {} settings: {}",
        missing.len(),
        if missing.len() == 1 { " is" } else { "s are" },
        if missing.len() == 1 { "its" } else { "their" },
        missing.join(", ")
    )
}

/// Reads a plug-in's state; its control half keeps it for a restart.
fn read_state(control: &mut Plugin) -> Done {
    Done::Saved(control.save_state().ok())
}

/// Starts a plug-in on a thread of its own and sends the result.
fn spawn_start(sender: Sender<Started>, key: u64, generation: u64, launch: Launch, state: Option<SavedState>) {
    let fallback = sender.clone();
    suhina_plugins::trace!(
        "rack starts key {key} generation {generation} ara {} (regions {})",
        launch.ara.is_some(),
        launch.ara.as_ref().map_or(0, |a| a.regions.len())
    );
    let spawned = std::thread::Builder::new().name("suhina-plugin-start".into()).spawn(move || {
        let mut result = start(launch, state).map_err(|e| e.to_string());
        suhina_plugins::trace!("rack started key {key} generation {generation}: {:?}", result.as_ref().map(|_| ()));
        let first_state = result.as_mut().ok().and_then(|(plugin, _, _)| plugin.save_state().ok());
        let _gone = sender.send(Started { key, generation, result, first_state });
    });
    if let Err(e) = spawned {
        let result = Err(format!("no thread to start it on: {e}"));
        let _gone = fallback.send(Started { key, generation, result, first_state: None });
    }
}

/// Starts the plug-in `launch` names and gives it `state`. A plug-in that refuses the state
/// still runs, with its own defaults; the reason comes back.
fn start(launch: Launch, state: Option<SavedState>) -> Result<(Plugin, PluginAudio, Option<String>), PluginError> {
    let (mut plugin, audio) = Plugin::start(launch)?;
    let mut refused = None;
    if let Some(state) = state {
        match plugin.load_state(state) {
            Ok(()) => {}
            Err(PluginError::Host(why)) => refused = Some(why),
            Err(other) => return Err(other),
        }
    }
    Ok((plugin, audio, refused))
}

#[cfg(test)]
#[path = "rack_tests.rs"]
mod tests;
