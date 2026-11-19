// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session-owned analysis jobs. Workers receive immutable PCM, never a project or file path.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};

use serde_json::{Value, json};
use suhina_instrument_analysis::{Outcome, Report};
use suhina_model::{AudioData, ChannelId, Device, DeviceId, DeviceKind, DeviceParam};

use crate::commands::{Args, device_channel, device_channel_read, with_override};
use crate::device_groups::device_path;
use crate::{CommandError, Session};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
const MAX_WORKERS: usize = 2;

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
    pub job: String,
    pub device: DeviceId,
    pub phase: AnalysisPhase,
    pub completed: usize,
    pub total: usize,
    pub report: Option<Report>,
    pub accepted_root: Option<u8>,
    pub message: Option<String>,
}

struct Source {
    channel: ChannelId,
    device: DeviceId,
    file: String,
    pcm: Arc<AudioData>,
    start: f64,
    end: f64,
    root: f64,
}

#[derive(Default)]
pub(crate) struct Jobs {
    current: Option<Job>,
}

struct Job {
    id: u64,
    source: Source,
    cancel: Arc<AtomicBool>,
    progress: Arc<[AtomicUsize; 2]>,
    receiver: Option<mpsc::Receiver<Result<Report, suhina_instrument_analysis::Error>>>,
    phase: AnalysisPhase,
    report: Option<Report>,
    message: Option<String>,
    pending_root: Option<u8>,
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
                        self.report = Some(report);
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
            job: self.id.to_string(),
            device: self.source.device,
            phase: self.phase,
            completed: self.progress[0].load(Ordering::Acquire),
            total: self.progress[1].load(Ordering::Acquire),
            report: self.report.clone(),
            message: self.message.clone(),
            accepted_root: (self.phase == AnalysisPhase::Accepted).then_some(self.source.root as u8),
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
                if let Some(pending) = job.pending_root.take() {
                    if root == f64::from(pending) {
                        job.source.root = root;
                        job.phase = AnalysisPhase::Accepted;
                    }
                    // A failed project transaction restored the original root; retain the proposal.
                }
                if root != job.source.root {
                    job.stop(AnalysisPhase::Stale, "the sample root changed; analyze again before accepting");
                }
            }
        }
        job.receive(false);
    }

    /// The latest job for this session, after a nonblocking poll.
    pub fn instrument_analysis_view(&mut self) -> Option<AnalysisView> {
        self.refresh_instrument_analysis();
        self.analysis.current.as_ref().map(Job::view)
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
    session.refresh_instrument_analysis();
    if session.analysis.current.as_ref().is_some_and(|j| j.receiver.is_some()) {
        return Err("the previous analysis is still running or cancelling".into());
    }
    let (channel, device) = selected(session, params)?;
    let file = device.sample_file().ok_or("load a sample before analyzing it")?.to_owned();
    let pcm = session.audio().files.get(&file).cloned().ok_or("the sample audio is missing or not prepared")?;
    let start = parameter(device, "start", 0.0);
    let end = parameter(device, "end", 1.0);
    let root = parameter(device, "root", 60.0);
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
    let source = Source { channel, device: device.id, file, pcm: pcm.clone(), start, end, root };
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
            let result = suhina_instrument_analysis::analyze(
                &pcm.samples,
                pcm.sample_rate,
                pcm.channels,
                range,
                |done, total| {
                    worker_progress[1].store(total, Ordering::Release);
                    worker_progress[0].store(done, Ordering::Release);
                    !worker_cancel.load(Ordering::Acquire)
                },
            );
            drop(pcm);
            drop(permit);
            let _ = send.send(result);
        })
        .map_err(|error| format!("could not start sample analysis: {error}"))?;
    drop(worker);
    session.analysis.current = Some(Job {
        id,
        source,
        cancel,
        progress,
        receiver: Some(receiver),
        phase: AnalysisPhase::Running,
        report: None,
        message: None,
        pending_root: None,
    });
    Ok(json!({"job":id.to_string(),"status":"running"}))
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
        "outcome":match r.outcome { Outcome::Stable=>"stable", Outcome::Silent=>"silent", Outcome::TooShort=>"tooShort", Outcome::NoStablePitch=>"noStablePitch" },
        "activeWindows":r.active_windows,"voicedWindows":r.voiced_windows,
        "proposal":r.proposal.map(|p| json!({"root":p.root,"midi":p.midi,"hz":p.hz,"cents":p.cents,"periodicity":p.periodicity,"spreadCents":p.spread_cents}))
    }));
    json!({"job":view.job,"deviceId":view.device.0,"status":view.phase.as_str(),"completed":view.completed,"total":view.total,"report":report,"acceptedRoot":view.accepted_root,"message":view.message})
}

pub(crate) fn cancel(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let job = matching_job(session, params)?;
    job.stop(AnalysisPhase::Cancelled, "analysis cancelled; the playable instrument is unchanged");
    Ok(view_json(&job.view()))
}

pub(crate) fn accept(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let job = matching_job(session, params)?;
    if job.phase != AnalysisPhase::Ready {
        return Err("only a completed, current analysis proposal can be accepted".into());
    }
    let proposal = job.report.as_ref().and_then(|r| r.proposal).ok_or("analysis found no stable root-note proposal")?;
    let root = Args::new(params).opt_index("root", "MIDI root key")?.unwrap_or(usize::from(proposal.root));
    let root = u8::try_from(root).ok().filter(|v| *v <= 127).ok_or("root must be an integer from 0 to 127")?;
    let channel = device_channel(session, params)?;
    let path = device_path(&channel.devices, params, "device")?;
    let device = suhina_model::device_at_mut(&mut channel.devices, &path).ok_or("the instrument is missing")?;
    if let Some(param) = device.params.iter_mut().find(|p| p.id == "root") {
        param.value = f64::from(root);
    } else {
        device.params.push(DeviceParam::new("root", f64::from(root)));
    }
    let overridden = crate::automation::override_lane(&mut device.automation, |lane| lane.param == "root");
    if let Some(job) = &mut session.analysis.current {
        job.pending_root = Some(root);
    }
    Ok(with_override(json!({"root":root,"accepted":true}), overridden))
}
