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
use crate::instrument_folder::FolderReport;
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
    /// A folder of one-note files fitted into one model; see instrument-modal-fitting.md.
    Folder,
}

impl AnalysisKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Modal => "modal",
            Self::Folder => "folder",
        }
    }
}

enum Prepared {
    Root(Report),
    Modal(Box<modal::Report>),
    Folder(Box<FolderReport>),
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
    /// The last audition's comparison of the fit with its source.
    pub comparison: Option<suhina_instrument_analysis::compare::Comparison>,
    /// A folder fit's files and recordings.
    pub folder: Option<FolderReport>,
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
    /// The last audition's level-matched comparison of the fit with its source.
    comparison: Option<suhina_instrument_analysis::compare::Comparison>,
    folder: Option<FolderReport>,
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
                            Prepared::Folder(report) => self.folder = Some(*report),
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
            comparison: self.comparison.clone(),
            folder: self.folder.clone(),
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
            let folder = job.kind == AnalysisKind::Folder;
            let matches_source = if folder {
                device.is_some_and(|d| {
                    matches!(&d.kind, DeviceKind::Builtin(id) if id == "instrument")
                        && d.instrument_modal_profile() == job.source.profile.as_deref()
                })
            } else {
                device.is_some_and(|d| {
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
                    .is_some_and(|audio| Arc::ptr_eq(audio, &job.source.pcm))
            };
            if !matches_source {
                job.stop(
                    AnalysisPhase::Stale,
                    if folder {
                        "the Instrument or its fitted model changed; fit the folder again"
                    } else {
                        "the device, source or trim changed; analyze the current source again"
                    },
                );
            } else if !folder && let Some(device) = device {
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
    spawn(session, kind, source, move |progress| {
        if kind == AnalysisKind::Root {
            suhina_instrument_analysis::analyze(&pcm.samples, pcm.sample_rate, pcm.channels, range, progress)
                .map(Prepared::Root)
        } else {
            modal::fit(&pcm.samples, pcm.sample_rate, pcm.channels, range, root.round() as u8, progress)
                .map(|r| Prepared::Modal(Box::new(r)))
        }
    })
}

/// `instrument.fitFolder`: fit a folder's one-note files into one model, on the worker.
pub(crate) fn start_folder(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let folder = Args::new(params).raw_str("path").ok_or("path must be a folder of audio files")?;
    let folder = crate::files::absolute(std::path::Path::new(folder));
    let files = crate::instrument_folder::list(&folder)?;
    start_listed(session, params, files, folder.display().to_string())
}

/// `instrument.fitFiles`: the same job on a list of audio files from anywhere.
pub(crate) fn start_files(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let paths: Vec<std::path::PathBuf> = params
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("paths must be a list of audio files")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(|p| crate::files::absolute(std::path::Path::new(p)))
                .ok_or("paths must be a list of audio files")
        })
        .collect::<Result<_, _>>()?;
    let files = crate::instrument_folder::listed(&paths)?;
    let label = format!("{} files", files.len());
    start_listed(session, params, files, label)
}

/// Start a folder job on `files` for the selected Instrument; `label` names them in the source.
fn start_listed(
    session: &mut Session,
    params: &Value,
    files: Vec<std::path::PathBuf>,
    label: String,
) -> Result<Value, CommandError> {
    session.refresh_instrument_analysis();
    if session.analysis.current.as_ref().is_some_and(|j| j.receiver.is_some()) {
        return Err("the previous analysis is still running or cancelling".into());
    }
    let (channel, device) = selected(session, params)?;
    if !matches!(&device.kind, DeviceKind::Builtin(id) if id == "instrument") {
        return Err("folder fitting needs an Instrument".into());
    }
    let (device_id, profile) = (device.id, device.instrument_modal_profile().map(<[u8]>::to_vec));
    let pcm = Arc::new(AudioData { sample_rate: 48000, channels: 1, samples: Vec::new() });
    let source = Source { channel, device: device_id, file: label, pcm, start: 0.0, end: 1.0, root: 60.0, profile };
    let count = files.len();
    let mut result = spawn(session, AnalysisKind::Folder, source, move |progress| {
        crate::instrument_folder::fit_folder(&files, progress).map(|r| Prepared::Folder(Box::new(r)))
    })?;
    result["files"] = json!(count);
    Ok(result)
}

type Progress<'a> = &'a mut dyn FnMut(usize, usize) -> bool;

/// Start `work` on a preparation worker within the admission budget, as the session's job.
fn spawn(
    session: &mut Session,
    kind: AnalysisKind,
    source: Source,
    work: impl FnOnce(Progress<'_>) -> Result<Prepared, suhina_instrument_analysis::Error> + Send + 'static,
) -> Result<Value, CommandError> {
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
            let mut progress = |done: usize, total: usize| -> bool {
                worker_progress[1].store(total, Ordering::Release);
                worker_progress[0].store(done, Ordering::Release);
                !worker_cancel.load(Ordering::Acquire)
            };
            // `work` owns its PCM and releases it when it returns.
            let result = work(&mut progress);
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
        comparison: None,
        folder: None,
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
        "outcome":match r.outcome { Outcome::Stable=>"stable", Outcome::Silent=>"silent", Outcome::TooShort=>"tooShort", Outcome::NoStablePitch=>"noStablePitch", Outcome::TonalCenter=>"tonalCenter" },
        "activeWindows":r.active_windows,"voicedWindows":r.voiced_windows,
        "notes":r.notes.iter().map(|n| json!({"startSeconds":n.start_seconds,"endSeconds":n.end_seconds,"midi":n.midi,"root":n.root,
            "periodicity":n.periodicity,"spreadCents":n.spread_cents,"share":n.share})).collect::<Vec<_>>(),
        "glide":r.glide.map(|g| json!({"fromMidi":g.from_midi,"seconds":g.seconds})),
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
            "attackSeconds":rec.attack.len() as f64 / f64::from(rec.attack_rate.max(1)),
            // The held layer of a bowed, blown or sung recording (instrument-sustained-models.md).
            "held":rec.sustain.map(|h| json!({"vibratoHz":h.vibrato_hz,"vibratoCents":h.vibrato_cents,
                "vibratoDelaySeconds":h.vibrato_delay,"vibratoFadeSeconds":h.vibrato_fade,"tremoloHz":h.tremolo_hz,
                "tremoloDepth":h.tremolo_depth,"driftCents":h.drift_cents,"releaseSeconds":h.release_seconds,
                "bodyFixed":h.body_fixed,"noiseBands":h.noise.iter().filter(|v| **v > 0.0).count(),
                "risingModes":h.rise.iter().filter(|v| **v > 0.0).count()}))})),
        // A chord split into one recording per note (instrument-modal-fitting.md).
        "notes":r.notes.iter().map(|n| json!({"key":n.key,"modeCount":n.modes.len(),"level":n.level})).collect::<Vec<_>>(),
        "recordingModes":r.recording_modes.iter().map(|m| json!({"frequencyHz":m.frequency_hz,"decaySeconds":m.decay_seconds,
            "fastDecaySeconds":m.fast_decay_seconds,"fastShare":m.fast_share,"beatHz":m.beat_hz,"beatShare":m.beat_share,
            "phase":m.phase,"sustained":m.sustained,"loose":m.loose})).collect::<Vec<_>>(),
        "modes":r.modes.iter().map(|m| json!({"frequencyHz":m.frequency_hz,"decaySeconds":m.decay_seconds,
            "estimationRmsDb":m.estimation_rms_db,"validationRmsDb":m.validation_rms_db,"sustained":m.sustained,
            "estimationWindows":m.estimation_windows,"validationWindows":m.validation_windows})).collect::<Vec<_>>()
    }));
    let folder = view.folder.as_ref().map(|f| {
        json!({
            "recordings":f.recordings.len(),
            "files":f.files.iter().map(|file| json!({"name":file.name,"key":file.key,"velocity":file.velocity,
                "outcome":file.outcome.as_str(),"reason":file.outcome.reason()})).collect::<Vec<_>>()
        })
    });
    json!({"job":view.job,"kind":view.kind.as_str(),"deviceId":view.device.0,"status":view.phase.as_str(),"completed":view.completed,"total":view.total,"modelRecordings":view.model_recordings,"report":report,"modal":modal,"folder":folder,"acceptedRoot":view.accepted_root,"message":view.message})
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
    // A single stable note, or else the tonal center of melodic or chordal material.
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
    if matching_job(session, params)?.kind == AnalysisKind::Folder {
        if Args::new(params).opt_bool("notes")?.unwrap_or(false) {
            return Err("notes is for a chord's fit, not a folder's".into());
        }
        return adopt_folder(session, params, false);
    }
    let job = matching_job(session, params)?;
    if job.kind != AnalysisKind::Modal || job.phase != AnalysisPhase::Ready {
        return Err("only a completed, current modal fit can be accepted".into());
    }
    let report = job.modal.as_ref().ok_or("the sample did not yield a decaying modal profile")?;
    let notes = Args::new(params).opt_bool("notes")?.unwrap_or(false);
    if notes && report.notes.len() < 2 {
        return Err("this fit found no chord to split into notes".into());
    }
    let chord = notes.then(|| report.notes.clone());
    let recording = report.recording.clone();
    let profile = report.profile.ok_or("the sample did not yield a decaying modal profile")?;
    let channel = device_channel(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at_mut(&mut channel.devices, &path).ok_or("the instrument is missing")?;
    // A fitted recording becomes a one-member revision-4 set; a fit that could not form one
    // (its level out of bounds) is still adopted as its revision-2 profile.
    let (state, overridden, result) = match (chord, recording) {
        // A chord's notes: one recording per key, as a new set.
        (Some(notes), _) => {
            let keys: Vec<u8> = notes.iter().map(|n| n.key).collect();
            let set = ModelSet::new(notes).map_err(|error| error.to_string())?;
            let bytes = set.encode();
            let overridden = crate::instrument_profile::apply_model_set(device, &bytes, true)?;
            let result = json!({"accepted":true,"recordings":keys.len(),"notes":keys,"revision":MODEL_REVISION,"source":"modal"});
            (bytes, overridden, result)
        }
        (None, Some(recording)) => {
            let attack = recording.attack.len() as f64 / f64::from(recording.attack_rate.max(1));
            let modes = recording.modes.len();
            let set = ModelSet::new(vec![recording]).map_err(|error| error.to_string())?;
            let bytes = set.encode();
            let overridden = crate::instrument_profile::apply_model_set(device, &bytes, true)?;
            let result = json!({"accepted":true,"modeCount":modes,"recordings":1,"attackSeconds":attack,"revision":MODEL_REVISION,"source":"modal"});
            (bytes, overridden, result)
        }
        (None, None) => {
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

/// `instrument.auditionFit`: play the source or the fitted model, level-matched, and compare them.
pub(crate) fn audition_fit(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let which = Args::new(params).text("which")?;
    if !matches!(which, "original" | "fitted") {
        return Err("which must be original or fitted".into());
    }
    let job = matching_job(session, params)?;
    if job.kind != AnalysisKind::Modal || !matches!(job.phase, AnalysisPhase::Ready | AnalysisPhase::Accepted) {
        return Err("only a completed modal fit can be auditioned".into());
    }
    let report = job.modal.as_ref().ok_or("this fit has no model to audition")?;
    let recording = report.recording.clone().ok_or("this fit has no model to audition")?;
    let pcm = job.source.pcm.clone();
    let channels = usize::from(pcm.channels).max(1);
    let channel = usize::from(report.channel);
    let original: Vec<f32> = pcm
        .samples
        .chunks_exact(channels)
        .skip(report.range.start)
        .take(report.range.end.saturating_sub(report.range.start))
        .map(|frame| frame.get(channel).copied().unwrap_or(0.0))
        .collect();
    let fitted = render_recording(&recording, pcm.sample_rate, original.len())?;
    let comparison = suhina_instrument_analysis::compare::compare(&original, &fitted, pcm.sample_rate)
        .ok_or("the fit and its source have too little sound to compare")?;
    job.comparison = Some(comparison.clone());
    let samples = if which == "original" {
        original
    } else {
        // Level-matched: the fitted model at the original's RMS, at most 24 dB apart.
        let gain = 10.0_f64.powf((-comparison.level_difference_db).clamp(-24.0, 24.0) / 20.0) as f32;
        fitted.iter().map(|x| x * gain).collect()
    };
    let seconds = samples.len() as f64 / f64::from(pcm.sample_rate.max(1));
    let data = std::sync::Arc::new(AudioData { sample_rate: pcm.sample_rate, channels: 1, samples });
    let preview_gain = 10.0_f64.powf(-6.0 / 20.0) as f32;
    let played = match session.player_mut() {
        Ok(player) => player.controller_mut().preview_sample(data, 0.0, preview_gain).is_ok(),
        Err(_) => false,
    };
    Ok(json!({
        "which": which, "played": played, "seconds": seconds,
        "levelDifferenceDb": comparison.level_difference_db, "meanDistanceDb": comparison.mean_distance_db,
        "bands": comparison.bands.iter().map(|b| json!({"centerHz": b.center_hz, "differenceDb": b.difference_db})).collect::<Vec<_>>()
    }))
}

/// The fitted recording as the native Instrument plays it: its key and velocity held for
/// `frames` at `sample_rate`, mono.
fn render_recording(
    recording: &suhina_dsp::modal::model::ModelRecording,
    sample_rate: u32,
    frames: usize,
) -> Result<Vec<f32>, CommandError> {
    use suhina_device_api::{Device as NativeDevice, NoteEvent, ProcessContext};
    let set = ModelSet::new(vec![recording.clone()]).map_err(|error| error.to_string())?;
    let mut device = suhina_device_registry::create("instrument", f64::from(sample_rate))
        .ok_or("the Instrument cannot render at this sample rate")?;
    NativeDevice::load_model_state(device.as_mut(), &set.encode())
        .map_err(|_| "the fitted model could not be loaded")?;
    for (id, value) in [("gain", 0.0), ("attack", 0.0), ("sustain", 1.0), ("source", 0.0)] {
        if let Some(index) = NativeDevice::info(device.as_ref()).param_index(id) {
            NativeDevice::set_param(device.as_mut(), index, value);
        }
    }
    NativeDevice::reset(device.as_mut());
    NativeDevice::event(device.as_mut(), NoteEvent::On { key: recording.key, velocity: recording.velocity });
    let mut output = vec![0.0f32; frames * 2];
    for block in output.chunks_mut(1024) {
        NativeDevice::process(device.as_mut(), &ProcessContext::default(), block);
    }
    Ok(output.chunks(2).map(|frame| frame.first().copied().unwrap_or(0.0)).collect())
}

/// `instrument.addFit`: the ready fit's recording joins the device's revision-4 set.
pub(crate) fn add_fit(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let velocity = match Args::new(params).opt_f64("velocity")? {
        None => suhina_instrument_analysis::modal::DEFAULT_VELOCITY,
        Some(v) if v.is_finite() && v > 0.0 && v <= 1.0 => v as f32,
        Some(_) => return Err("velocity must be a number above 0 and at most 1".into()),
    };
    if matching_job(session, params)?.kind == AnalysisKind::Folder {
        return adopt_folder(session, params, true);
    }
    let source = source_param(params)?;
    let new_source = new_source_param(params)?;
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
    let existing = ModelSet::decode(existing).map_err(|error| error.to_string())?;
    let source = if new_source { next_source(&existing)? } else { source };
    recording.source = source;
    let set = existing.with(recording).map_err(|error| error.to_string())?;
    let bytes = set.encode();
    let overridden = crate::instrument_profile::apply_model_set(device, &bytes, false)?;
    let count = set.recordings().len();
    if let Some(job) = &mut session.analysis.current {
        job.pending_state = Some(bytes);
    }
    Ok(with_override(
        json!({"added":true,"recordings":count,"key":key,"velocity":velocity,"recordingSource":source,"modeCount":modes,"revision":MODEL_REVISION,"source":"modal"}),
        overridden,
    ))
}

/// Whether `newSource` asks for a source of its own; it and `source` exclude each other.
fn new_source_param(params: &Value) -> Result<bool, CommandError> {
    let new_source = Args::new(params).opt_bool("newSource")?.unwrap_or(false);
    if new_source && Args::new(params).has("source") {
        return Err("give either source or newSource, not both".into());
    }
    Ok(new_source)
}

/// One more than the highest source a set uses: another instrument's recordings.
fn next_source(set: &ModelSet) -> Result<u8, CommandError> {
    let highest = set.recordings().iter().map(|r| r.source).max().unwrap_or(0);
    highest
        .checked_add(1)
        .filter(|n| *n < 16)
        .ok_or_else(|| CommandError::from("the model already uses source 15, the last one"))
}

/// The optional `source` (instrument) number a fit is added as, 0 when absent.
fn source_param(params: &Value) -> Result<u8, CommandError> {
    match Args::new(params).opt_index("source", "source number, 0 to 15")? {
        None => Ok(0),
        Some(n) => Ok(u8::try_from(n).ok().filter(|n| *n < 16).ok_or("source must be a whole number from 0 to 15")?),
    }
}

/// Accept (a new set) or add (into the device's set, all or none) a folder fit's recordings.
fn adopt_folder(session: &mut Session, params: &Value, add: bool) -> Result<Value, CommandError> {
    if add && Args::new(params).has("velocity") {
        return Err("a folder's velocities come from its layers; leave velocity out".into());
    }
    let source = source_param(params)?;
    let new_source = new_source_param(params)?;
    if new_source && !add {
        return Err("newSource is for adding to a model".into());
    }
    let job = matching_job(session, params)?;
    if job.phase != AnalysisPhase::Ready {
        return Err("only a completed, current folder fit can be accepted or added".into());
    }
    let mut recordings = job.folder.as_ref().map(|f| f.recordings.clone()).unwrap_or_default();
    if recordings.is_empty() {
        return Err("no file in the folder gave a fitted recording".into());
    }
    let channel = device_channel(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at_mut(&mut channel.devices, &path).ok_or("the instrument is missing")?;
    let (set, source) = if add {
        let existing = device
            .instrument_modal_profile()
            .filter(|bytes| bytes.starts_with(MODEL_MAGIC))
            .ok_or("add needs an Instrument with an accepted fitted model; accept one first")?;
        let mut set = ModelSet::decode(existing).map_err(|error| error.to_string())?;
        let source = if new_source { next_source(&set)? } else { source };
        for mut recording in recordings {
            recording.source = source;
            set = set.with(recording).map_err(|error| format!("nothing was added: {error}"))?;
        }
        (set, source)
    } else {
        for recording in &mut recordings {
            recording.source = source;
        }
        (ModelSet::new(recordings).map_err(|error| error.to_string())?, source)
    };
    let bytes = set.encode();
    let overridden = crate::instrument_profile::apply_model_set(device, &bytes, !add)?;
    let count = set.recordings().len();
    if let Some(job) = &mut session.analysis.current {
        job.pending_state = Some(bytes);
    }
    let verb = if add { "added" } else { "accepted" };
    Ok(with_override(
        json!({verb:true,"recordings":count,"recordingSource":source,"revision":MODEL_REVISION,"source":"modal"}),
        overridden,
    ))
}
