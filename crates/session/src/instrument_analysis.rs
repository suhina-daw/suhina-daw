// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session-owned analysis jobs. Workers receive immutable PCM, never a project or file path.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};

use serde_json::{Value, json};
use suhina_dsp::modal::model::{MODEL_MAGIC, MODEL_REVISION, ModelSet};
use suhina_instrument_analysis::{Outcome, Report, modal};
use suhina_model::{AudioData, ChannelId, Device, DeviceId, DeviceKind};

use crate::commands::{Args, device_channel, device_channel_read, with_override};
use crate::device_groups::device_path;
use crate::{CommandError, Session};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
const MAX_WORKERS: usize = 2;

#[cfg(test)]
#[path = "instrument_analysis_tests.rs"]
mod tests;

struct Permit;
impl Permit {
    fn acquire() -> Result<Self, CommandError> {
        WORKERS
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < MAX_WORKERS).then_some(n + 1))
            .map(|_| Self)
            .map_err(|_| "sample analysis workers are busy; wait for the current jobs to finish".into())
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisPhase {
    Running,
    Ready,
    Accepted,
    Cancelled,
    Stale,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisKind {
    Root,
    Modal,
}

impl AnalysisKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Modal => "modal",
        }
    }
}

enum Prepared {
    Root(Report),
    Modal(Box<modal::Report>),
}

impl AnalysisPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Ready => "ready",
            Self::Accepted => "accepted",
            Self::Cancelled => "cancelled",
            Self::Stale => "stale",
            Self::Failed => "failed",
        }
    }
}

/// A bounded snapshot for native UI/CLI clients, separate from saved instrument parameters.
#[derive(Clone, Debug)]
pub struct AnalysisView {
    pub kind: AnalysisKind,
    pub job: String,
    pub device: DeviceId,
    pub phase: AnalysisPhase,
    pub completed: usize,
    pub total: usize,
    pub report: Option<Report>,
    pub modal: Option<modal::Report>,
    pub accepted_root: Option<u8>,
    pub message: Option<String>,
    /// Recordings in the target Instrument's fitted model now; None without a revision-4 set.
    pub model_recordings: Option<usize>,
}

struct Source {
    channel: ChannelId,
    device: DeviceId,
    file: String,
    pcm: Arc<AudioData>,
    start: f64,
    end: f64,
    root: f64,
    profile: Option<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct Jobs {
    current: Option<Job>,
}

struct Job {
    kind: AnalysisKind,
    id: u64,
    source: Source,
    cancel: Arc<AtomicBool>,
    progress: Arc<[AtomicUsize; 2]>,
    receiver: Option<mpsc::Receiver<Result<Prepared, suhina_instrument_analysis::Error>>>,
    phase: AnalysisPhase,
    report: Option<Report>,
    modal: Option<modal::Report>,
    message: Option<String>,
    pending_root: Option<u8>,
    /// Model bytes an accept or add is writing; adopted as the job's state once it succeeds.
    pending_state: Option<Vec<u8>>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Job {
    fn stop(&mut self, phase: AnalysisPhase, message: &str) {
        self.cancel.store(true, Ordering::Release);
        self.phase = phase;
        self.message = Some(message.to_owned());
    }

    fn receive(&mut self, wait: bool) {
        let Some(rx) = &self.receiver else { return };
        let result = if wait {
            rx.recv().map(Some).map_err(|_| ())
        } else {
            match rx.try_recv() {
                Ok(result) => Ok(Some(result)),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => Err(()),
            }
        };
        match result {
            Ok(None) => (),
            Ok(Some(result)) => {
                self.receiver = None;
                if self.phase != AnalysisPhase::Running {
                    return;
                }
                match result {
                    Ok(report) => {
                        match report {
                            Prepared::Root(report) => self.report = Some(report),
                            Prepared::Modal(report) => self.modal = Some(*report),
                        }
                        self.phase = AnalysisPhase::Ready;
                    }
                    Err(error) => {
                        self.phase = if error == suhina_instrument_analysis::Error::Cancelled {
                            AnalysisPhase::Cancelled
                        } else {
                            AnalysisPhase::Failed
                        };
                        self.message = Some(error.to_string());
                    }
                }
            }
            Err(()) => {
                self.receiver = None;
                if self.phase == AnalysisPhase::Running {
                    self.phase = AnalysisPhase::Failed;
                    self.message = Some("the analysis worker ended without a result".to_owned());
                }
            }
        }
    }

    fn view(&self) -> AnalysisView {
        AnalysisView {
            kind: self.kind,
            job: self.id.to_string(),
            device: self.source.device,
            phase: self.phase,
            completed: self.progress[0].load(Ordering::Acquire),
            total: self.progress[1].load(Ordering::Acquire),
            report: self.report.clone(),
            modal: self.modal.clone(),
            message: self.message.clone(),
            accepted_root: (self.phase == AnalysisPhase::Accepted && self.kind == AnalysisKind::Root)
                .then_some(self.source.root as u8),
            model_recordings: None,
        }
    }
}

fn parameter(device: &Device, id: &str, default: f64) -> f64 {
    device.params.iter().find(|p| p.id == id).map_or(default, |p| p.value)
}

fn selected<'a>(session: &'a Session, params: &Value) -> Result<(ChannelId, &'a Device), CommandError> {
    let channel = device_channel_read(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at(&channel.devices, &path).ok_or("the instrument is missing")?;
    if !matches!(&device.kind, DeviceKind::Builtin(id) if matches!(id.as_str(), "instrument" | "sampler")) {
        return Err("sample analysis needs an Instrument or Sampler".into());
    }
    Ok((channel.id, device))
}

impl Session {
    /// Polls without waiting; also cancels invalidated targets. Called after every command.
    pub fn refresh_instrument_analysis(&mut self) {
        let Some(job) = &mut self.analysis.current else { return };
        if !matches!(job.phase, AnalysisPhase::Cancelled | AnalysisPhase::Stale | AnalysisPhase::Failed) {
            let device = self.project.channel(job.source.channel).and_then(|channel| {
                suhina_model::walk_devices(&channel.devices)
                    .into_iter()
                    .find(|(_, d)| d.id == job.source.device)
                    .map(|(_, d)| d)
            });
            let matches_source = device.is_some_and(|d| {
                matches!(&d.kind, DeviceKind::Builtin(id) if matches!(id.as_str(), "instrument" | "sampler"))
                    && d.sample_file() == Some(job.source.file.as_str())
                    && parameter(d, "start", 0.0) == job.source.start
                    && parameter(d, "end", 1.0) == job.source.end
                    && (job.kind != AnalysisKind::Modal
                        || (d.instrument_modal_profile() == job.source.profile.as_deref()
                            && matches!(&d.kind, DeviceKind::Builtin(id) if id == "instrument")))
            }) && self
                .audio
                .pool
                .files
                .get(&job.source.file)
                .is_some_and(|audio| Arc::ptr_eq(audio, &job.source.pcm));
            if !matches_source {
                job.stop(AnalysisPhase::Stale, "the device, source or trim changed; analyze the current source again");
            } else if let Some(device) = device {
                let root = parameter(device, "root", 60.0);
                if root != job.source.root {
                    job.stop(AnalysisPhase::Stale, "the sample root changed; analyze again before accepting");
                }
            }
        }
        job.receive(false);
    }

    /// Publish adoption only after the editing transaction succeeds, even when the corrected
    /// root equals its old value. A failed validation must not consume a ready proposal.
    pub(crate) fn finish_instrument_analysis_command(&mut self, succeeded: bool) {
        if let Some(job) = &mut self.analysis.current
            && let Some(root) = job.pending_root.take()
            && succeeded
        {
            job.source.root = f64::from(root);
            job.phase = AnalysisPhase::Accepted;
        }
        if let Some(job) = &mut self.analysis.current
            && let Some(state) = job.pending_state.take()
            && succeeded
        {
            job.source.profile = Some(state);
            job.phase = AnalysisPhase::Accepted;
        }
    }

    /// The latest job for this session, after a nonblocking poll.
    pub fn instrument_analysis_view(&mut self) -> Option<AnalysisView> {
        self.refresh_instrument_analysis();
        let job = self.analysis.current.as_ref()?;
        let mut view = job.view();
        // Only the set's header is read: the UI asks every frame.
        view.model_recordings = self.project.channel(job.source.channel).and_then(|channel| {
            suhina_model::walk_devices(&channel.devices)
                .into_iter()
                .find(|(_, d)| d.id == job.source.device)
                .and_then(|(_, d)| d.instrument_modal_profile())
                .and_then(ModelSet::recording_count)
        });
        Some(view)
    }

    /// Explicit completion barrier for offline clients and deterministic tests; the UI never calls it.
    pub fn sync_instrument_analysis(&mut self) {
        if let Some(job) = &mut self.analysis.current {
            job.receive(true);
        }
        self.refresh_instrument_analysis();
    }
}

pub(crate) fn start(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    start_kind(session, params, AnalysisKind::Root)
}

pub(crate) fn start_fit(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    start_kind(session, params, AnalysisKind::Modal)
}

fn start_kind(session: &mut Session, params: &Value, kind: AnalysisKind) -> Result<Value, CommandError> {
    session.refresh_instrument_analysis();
    if session.analysis.current.as_ref().is_some_and(|j| j.receiver.is_some()) {
        return Err("the previous analysis is still running or cancelling".into());
    }
    let (channel, device) = selected(session, params)?;
    if kind == AnalysisKind::Modal && !matches!(&device.kind, DeviceKind::Builtin(id) if id == "instrument") {
        return Err("modal fitting needs an Instrument".into());
    }
    let file = device.sample_file().ok_or("load a sample before analyzing it")?.to_owned();
    let pcm = session.audio().files.get(&file).cloned().ok_or("the sample audio is missing or not prepared")?;
    let start = parameter(device, "start", 0.0);
    let end = parameter(device, "end", 1.0);
    let root = parameter(device, "root", 60.0);
    if kind == AnalysisKind::Modal && root.fract() != 0.0 {
        return Err("choose an integer sample root key before fitting".into());
    }
    if !start.is_finite()
        || !end.is_finite()
        || !(0.0..=1.0).contains(&start)
        || !(0.0..=1.0).contains(&end)
        || start >= end
        || !root.is_finite()
        || !(0.0..=127.0).contains(&root)
    {
        return Err("the sample trim or root is invalid".into());
    }
    if !(1..=2).contains(&pcm.channels)
        || !(1000..=384000).contains(&pcm.sample_rate)
        || !pcm.samples.len().is_multiple_of(usize::from(pcm.channels))
    {
        return Err("analysis needs prepared mono or stereo audio at 1000..384000 Hz".into());
    }
    let range = (start * pcm.frames() as f64).floor() as usize..(end * pcm.frames() as f64).ceil() as usize;
    if range.is_empty() {
        return Err("the sample trim contains no audio frames".into());
    }
    let profile =
        if kind == AnalysisKind::Modal { device.instrument_modal_profile().map(<[u8]>::to_vec) } else { None };
    let source = Source { channel, device: device.id, file, pcm: pcm.clone(), start, end, root, profile };
    let permit = Permit::acquire()?;
    let id = NEXT_JOB
        .try_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
        .map_err(|_| CommandError::from("sample analysis job identities are exhausted"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let progress = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
    let (send, receiver) = mpsc::sync_channel(1);
    let worker_cancel = cancel.clone();
    let worker_progress = progress.clone();
    // Dropping the handle detaches it: no UI path joins. Admission includes cancelled work
    // until its PCM is released, before the explicit completion signal is sent.
    let worker = std::thread::Builder::new()
        .name("instrument-analysis".into())
        .spawn(move || {
            let progress = |done, total| {
                worker_progress[1].store(total, Ordering::Release);
                worker_progress[0].store(done, Ordering::Release);
                !worker_cancel.load(Ordering::Acquire)
            };
            let result = match kind {
                AnalysisKind::Root => {
                    suhina_instrument_analysis::analyze(&pcm.samples, pcm.sample_rate, pcm.channels, range, progress)
                        .map(Prepared::Root)
                }
                AnalysisKind::Modal => {
                    modal::fit(&pcm.samples, pcm.sample_rate, pcm.channels, range, root.round() as u8, progress)
                        .map(|r| Prepared::Modal(Box::new(r)))
                }
            };
            drop(pcm);
            drop(permit);
            let _ = send.send(result);
        })
        .map_err(|error| format!("could not start sample analysis: {error}"))?;
    drop(worker);
    session.analysis.current = Some(Job {
        kind,
        id,
        source,
        cancel,
        progress,
        receiver: Some(receiver),
        phase: AnalysisPhase::Running,
        report: None,
        modal: None,
        message: None,
        pending_root: None,
        pending_state: None,
    });
    Ok(json!({"job":id.to_string(),"kind":kind.as_str(),"status":"running"}))
}

fn matching_job<'a>(session: &'a mut Session, params: &Value) -> Result<&'a mut Job, CommandError> {
    session.refresh_instrument_analysis();
    let (channel, device) = selected(session, params)?;
    let device = device.id;
    let job = session.analysis.current.as_mut().ok_or("this session has no sample analysis")?;
    if channel != job.source.channel || device != job.source.device {
        return Err("the analysis belongs to a different instrument".into());
    }
    if Args::new(params).has("job") && Args::new(params).text("job")? != job.id.to_string() {
        return Err("the analysis job id is no longer current".into());
    }
    Ok(job)
}

pub(crate) fn status(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let job = matching_job(session, params)?;
    Ok(view_json(&job.view()))
}

fn view_json(view: &AnalysisView) -> Value {
    let report = view.report.as_ref().map(|r| json!({
        "version":r.version, "firstFrame":r.range.start, "endFrame":r.range.end,
        "sourceAssumption":if r.key.is_some() { "tonalMaterial" } else { "singleIsolatedPitchedNote" }, "polyphonyAssessed":false,
        "outcome":match r.outcome { Outcome::Stable=>"stable", Outcome::Silent=>"silent", Outcome::TooShort=>"tooShort", Outcome::NoStablePitch=>"noStablePitch", Outcome::TonalCentre=>"tonalCentre" },
        "activeWindows":r.active_windows,"voicedWindows":r.voiced_windows,
        "proposal":r.proposal.map(|p| json!({"root":p.root,"midi":p.midi,"hz":p.hz,"cents":p.cents,"periodicity":p.periodicity,"spreadCents":p.spread_cents})),
        "key":r.key.map(|k| {
            let mut key = key_candidate_json(&k.key);
            key["root"] = json!(k.root);
            key["runnerUp"] = key_candidate_json(&k.runner_up);
            key["tuningCents"] = json!(k.tuning_cents);
            key["medianMidi"] = json!(k.median_midi);
            key["windows"] = json!(k.windows);
            key
        })
    }));
    let modal = view.modal.as_ref().map(|r| json!({
        "version":r.version,"firstFrame":r.range.start,"endFrame":r.range.end,"channel":r.channel,
        "frameSize":r.frame_size,"analyzedSeconds":r.analyzed_seconds,
        "sourceAssumption":"singleIsolatedDecayingStrike","phaseReconstructed":r.recording.is_some(),"validationScope":"sameRecordingTemporalEnvelopes",
        "outcome":match r.outcome { modal::Outcome::Fitted=>"fitted",modal::Outcome::Silent=>"silent",modal::Outcome::TooShort=>"tooShort",modal::Outcome::NoDecayingModes=>"noDecayingModes" },
        "referenceKey":r.profile.map(|p|p.reference_key()),"referenceHz":r.profile.map(|p|p.reference_hz()),
        "recording":r.recording.as_ref().map(|rec| json!({"key":rec.key,"level":rec.level,"modeCount":rec.modes.len(),
            "attackSeconds":rec.attack.len() as f64 / f64::from(rec.attack_rate.max(1))})),
        "recordingModes":r.recording_modes.iter().map(|m| json!({"frequencyHz":m.frequency_hz,"decaySeconds":m.decay_seconds,
            "fastDecaySeconds":m.fast_decay_seconds,"fastShare":m.fast_share,"beatHz":m.beat_hz,"beatShare":m.beat_share,
            "phase":m.phase,"sustained":m.sustained})).collect::<Vec<_>>(),
        "modes":r.modes.iter().map(|m| json!({"frequencyHz":m.frequency_hz,"decaySeconds":m.decay_seconds,
            "estimationRmsDb":m.estimation_rms_db,"validationRmsDb":m.validation_rms_db,"sustained":m.sustained,
            "estimationWindows":m.estimation_windows,"validationWindows":m.validation_windows})).collect::<Vec<_>>()
    }));
    json!({"job":view.job,"kind":view.kind.as_str(),"deviceId":view.device.0,"status":view.phase.as_str(),"completed":view.completed,"total":view.total,"modelRecordings":view.model_recordings,"report":report,"modal":modal,"acceptedRoot":view.accepted_root,"message":view.message})
}

/// A key by its tonic's name (sharps) and mode; the correlation is a profile fit, not a probability.
fn key_candidate_json(key: &suhina_instrument_analysis::KeyCandidate) -> Value {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let mode = match key.mode {
        suhina_instrument_analysis::KeyMode::Major => "major",
        suhina_instrument_analysis::KeyMode::Minor => "minor",
    };
    json!({"tonic":NAMES.get(usize::from(key.tonic)).copied().unwrap_or("?"),"mode":mode,"correlation":key.correlation})
}

pub(crate) fn cancel(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let job = matching_job(session, params)?;
    job.stop(AnalysisPhase::Cancelled, "analysis cancelled; the playable instrument is unchanged");
    Ok(view_json(&job.view()))
}

pub(crate) fn accept(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let job = matching_job(session, params)?;
    if job.kind != AnalysisKind::Root {
        return Err("this is a modal fitting result; use instrument.acceptFit".into());
    }
    if job.phase != AnalysisPhase::Ready {
        return Err("only a completed, current analysis proposal can be accepted".into());
    }
    // A single stable note, or else the tonal centre of melodic or chordal material.
    let proposed = job
        .report
        .as_ref()
        .and_then(|r| r.proposal.map(|p| p.root).or(r.key.map(|k| k.root)))
        .ok_or("analysis found no root-note proposal")?;
    let root = Args::new(params).opt_index("root", "MIDI root key")?.unwrap_or(usize::from(proposed));
    let root = u8::try_from(root).ok().filter(|v| *v <= 127).ok_or("root must be an integer from 0 to 127")?;
    // Reuse the actual manual-edit path, including parameter messages to the running graph.
    // Merely assigning the model value would request an unnecessary graph rebuild.
    let mut edit = params.clone();
    edit["param"] = json!("root");
    edit["value"] = json!(root);
    let mut result = crate::devices::set_param(session, &edit)?;
    result["root"] = json!(root);
    result["accepted"] = json!(true);
    if let Some(job) = &mut session.analysis.current {
        job.pending_root = Some(root);
    }
    Ok(result)
}

pub(crate) fn accept_fit(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let job = matching_job(session, params)?;
    if job.kind != AnalysisKind::Modal || job.phase != AnalysisPhase::Ready {
        return Err("only a completed, current modal fit can be accepted".into());
    }
    let report = job.modal.as_ref().ok_or("the sample did not yield a decaying modal profile")?;
    let recording = report.recording.clone();
    let profile = report.profile.ok_or("the sample did not yield a decaying modal profile")?;
    let channel = device_channel(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at_mut(&mut channel.devices, &path).ok_or("the instrument is missing")?;
    // A fitted recording becomes a one-member revision-4 set; a fit that could not form one
    // (its level out of bounds) is still adopted as its revision-2 profile.
    let (state, overridden, result) = match recording {
        Some(recording) => {
            let attack = recording.attack.len() as f64 / f64::from(recording.attack_rate.max(1));
            let modes = recording.modes.len();
            let set = ModelSet::new(vec![recording]).map_err(|error| error.to_string())?;
            let bytes = set.encode();
            let overridden = crate::instrument_profile::apply_model_set(device, &bytes, true)?;
            let result = json!({"accepted":true,"modeCount":modes,"recordings":1,"attackSeconds":attack,"revision":MODEL_REVISION,"source":"modal"});
            (bytes, overridden, result)
        }
        None => {
            let overridden = crate::instrument_profile::apply_profile(
                device,
                &profile,
                crate::instrument_profile::ProfileSource::Modal,
            )?;
            let result =
                json!({"accepted":true,"modeCount":profile.modes().len(),"recordings":0,"revision":2,"source":"modal"});
            (profile.encode().to_vec(), overridden, result)
        }
    };
    if let Some(job) = &mut session.analysis.current {
        job.pending_state = Some(state);
    }
    Ok(with_override(result, overridden))
}

/// `instrument.addFit`: the ready fit's recording joins the device's revision-4 set.
pub(crate) fn add_fit(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let velocity = match Args::new(params).opt_f64("velocity")? {
        None => suhina_instrument_analysis::modal::DEFAULT_VELOCITY,
        Some(v) if v.is_finite() && v > 0.0 && v <= 1.0 => v as f32,
        Some(_) => return Err("velocity must be a number above 0 and at most 1".into()),
    };
    let job = matching_job(session, params)?;
    if job.kind != AnalysisKind::Modal || job.phase != AnalysisPhase::Ready {
        return Err("only a completed, current modal fit can be added".into());
    }
    let mut recording = job
        .modal
        .as_ref()
        .and_then(|r| r.recording.clone())
        .ok_or("the sample did not yield a fitted recording to add")?;
    recording.velocity = velocity;
    let (key, modes) = (recording.key, recording.modes.len());
    let channel = device_channel(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at_mut(&mut channel.devices, &path).ok_or("the instrument is missing")?;
    let existing = device
        .instrument_modal_profile()
        .filter(|bytes| bytes.starts_with(MODEL_MAGIC))
        .ok_or("add needs an Instrument with an accepted fitted model; accept one first")?;
    let set = ModelSet::decode(existing).and_then(|set| set.with(recording)).map_err(|error| error.to_string())?;
    let bytes = set.encode();
    let overridden = crate::instrument_profile::apply_model_set(device, &bytes, false)?;
    let count = set.recordings().len();
    if let Some(job) = &mut session.analysis.current {
        job.pending_state = Some(bytes);
    }
    Ok(with_override(
        json!({"added":true,"recordings":count,"key":key,"velocity":velocity,"modeCount":modes,"revision":MODEL_REVISION,"source":"modal"}),
        overridden,
    ))
}
