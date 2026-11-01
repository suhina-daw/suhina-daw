// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`Graph`]: everything a renderer derives from a project, and [`Graph::build`], which derives
//! it (#764 › 5). An edit builds a new graph off the audio thread and the renderer swaps it in
//! (ADR 0005).

use super::*;

/// Everything a renderer derives from the project. An edit builds a new one off the audio thread
/// and the renderer swaps it in (ADR 0005).
pub(crate) struct Graph {
    /// The tracks that hear the audio input, and whether each is heard this block.
    pub(crate) inputs: Vec<input::InputRoute>,
    pub(crate) inputs_heard: Vec<bool>,
    pub(crate) events: Vec<NoteEvent>,
    pub(crate) audio_events: Vec<ArrangedAudio>,
    /// Warped clips stretched as they play (ADR 0003), rendered ahead and mixed block by block.
    pub(crate) stretch_events: Vec<StretchEvent>,
    /// The ARA clips a clip host plays, by start (#990).
    pub(crate) hosted: Vec<HostedClip>,
    pub(crate) ahead: AheadReader,
    pub(crate) length: u64,
    pub(crate) launcher: Launcher,
    /// The tempo map counted in frames: every beat of this graph is placed through it
    /// (`docs/spec/tempo-map.md`).
    pub(crate) tempo: Arc<TempoMap>,
    /// The span playback repeats, in beats (`Transport::looping`): the transport turns them
    /// into frames through the tempo map as it plays, so the loop keeps its beats when the
    /// tempo changes.
    pub(crate) loop_beats: Option<(f64, f64)>,
    /// Device chains of the tracks that have devices (ADR 0004).
    pub(crate) chains: Vec<Chain>,
    /// The source render's capture, when the graph was built for one ([`RenderOptions::source`]).
    pub(crate) capture: Option<GraphCapture>,
    /// Every track's bus and the gains its sources are mixed at (muted tracks at zero), so
    /// sounding voices follow an edit to their track's mix.
    pub(crate) routes: Vec<(TrackId, u32, (f32, f32))>,
    /// Latency compensation: the most delay any chain adds, in frames, and the delay of
    /// everything that does not go through a chain (each chain delays itself up to it).
    pub(crate) latency: u32,
    pub(crate) direct_delay: Delay,
    /// Every track's block of sound, for its level meter.
    pub(crate) levels: meters::Levels,
}

impl Graph {
    /// Builds the graph for `project`: validates it and prepares every event and voice. Allocates;
    /// call it off the audio thread. The file-backed devices' files come from `assets`. With
    /// `source`, the graph is built for a source render: the node has a chain even without
    /// devices ([`RenderOptions::source`]).
    pub(crate) fn build(
        project: &Project,
        audio: &AudioPool,
        assets: &crate::DeviceAssets,
        sample_rate: u32,
        plugins: &mut Plugins,
        source: Option<TrackId>,
    ) -> Result<Box<Self>, RenderError> {
        if !(8_000..=384_000).contains(&sample_rate) {
            return Err(RenderError::InvalidSampleRate(sample_rate));
        }
        let build_began = std::time::Instant::now();
        let bpm = project.transport.tempo_bpm;
        if !bpm.is_finite() || !(1.0..=999.0).contains(&bpm) {
            return Err(RenderError::InvalidTempo(bpm));
        }
        project.validate().map_err(RenderError::InvalidProject)?;
        if let Some(track) = source
            && !project.tracks.iter().any(|t| t.id == track && t.channel.is_some())
        {
            return Err(RenderError::UnknownSource(track));
        }
        // Mixer parameters that follow lanes are built at their neutral values and applied
        // after the track's chain, frame by frame (#395).
        let neutral = automation::neutral(project);
        let project = neutral.as_ref().unwrap_or(project);
        // A source render is one node's mix, and nothing after the node is in it: the graph is
        // built from a copy whose channels above the source — its groups and the master — are
        // at unity and unmuted, so their faders are not in the node's file. The node's own
        // channel, and every channel under it, keep theirs.
        let above;
        let project = match source {
            Some(track) => {
                above = source_rest(project, track);
                &above
            }
            None => project,
        };

        let tempo = Arc::new(project.transport.tempo_map(f64::from(sample_rate)));
        let frames_per_beat = tempo.start_per_beat();
        // Audio events follow the map only when it changes, so a constant tempo plays exactly as
        // it always did.
        let changing = (!tempo.is_constant()).then(|| Arc::clone(&tempo));
        let column_of = |track: TrackId| {
            project
                .launcher
                .columns
                .iter()
                .position(|c| c.track == track)
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(NO_COLUMN)
        };
        // Device chains, group buses and latency compensation: every track is heard as late as
        // the path whose devices delay it most.
        // The tempo map as a number, for the stretch voices' keys: a voice on another map plays
        // other frames.
        let tempo_key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", *tempo).hash(&mut hasher);
            hasher.finish()
        };
        let chains_began = std::time::Instant::now();
        let (chains, latency) = buses::build(project, audio, sample_rate, plugins, column_of, assets, source);
        let chains_took = chains_began.elapsed();
        let mut voices_took = std::time::Duration::ZERO;
        // The source render's capture: the node's chain, its subtree's chains and a block
        // buffer for the sum at the node.
        let capture = source.map(|track| GraphCapture {
            chain: chains.iter().position(|c| c.track == track).unwrap_or(usize::MAX),
            keep: chains
                .iter()
                .map(|c| c.track == track || buses::in_source(project, c.track, node_channel(project, track)))
                .collect(),
            buffer: vec![0.0; MAX_BLOCK * 2],
        });
        let mut events = Vec::new();
        let mut audio_events = Vec::new();
        let mut stretch_events = Vec::new();
        let mut stretch_voices = Vec::new();
        let mut stretch_heard = Vec::new();
        let mut hosted = Vec::new();
        for (lane_index, lane) in project.arrangement.lanes.iter().enumerate() {
            for (clip_index, placed) in lane.clips.iter().enumerate().filter(|(_, c)| c.enabled) {
                // A part of a comp crossfades with a part of another take it meets (#989).
                let crossfaded = crossfade::crossfaded(project, lane, placed, bpm);
                let clip = crossfaded.as_ref();
                let Some(content) = project.content(clip.content) else { continue };
                for (content_index, content_lane) in content.lanes.iter().enumerate() {
                    let Some(track) = content_lane.target.or(lane.track) else { continue };
                    let Some((bus, (gain_left, gain_right))) = route(project, &chains, track) else { continue };
                    let column = column_of(track);
                    let meter = meters::meter_slot(project, track);
                    match &content_lane.data {
                        LaneData::Notes(_) => collect_notes(
                            clip,
                            content_lane,
                            &tempo,
                            (track, gain_left, gain_right, column, bus, meter),
                            musical_scale(content.scale),
                            &mut events,
                        ),
                        LaneData::Audio(region) => {
                            let Some(data) = audio.files.get(region.pool_key().as_ref()) else { continue };
                            let gains = (gain_left * AUDIO_HEADROOM, gain_right * AUDIO_HEADROOM);
                            // A track whose chain has a clip host: its ARA clips are the host's to
                            // play, at their gains and fades; its other clips go past it (#990).
                            let has_host = usize::try_from(bus)
                                .ok()
                                .and_then(|b| b.checked_sub(1))
                                .and_then(|i| chains.get(i))
                                .is_some_and(|c| c.ara.is_some());
                            if has_host && clip.ara {
                                let gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
                                let to_frame = |beats: f64| tempo::frame_of(&tempo, beats);
                                hosted.push(HostedClip {
                                    start: to_frame(clip.start),
                                    end: to_frame(clip.start + clip.length),
                                    gain_left: gains.0 * gain,
                                    gain_right: gains.1 * gain,
                                    fades: Fades::on_map(clip, &tempo, sample_rate),
                                    column,
                                    bus,
                                });
                                continue;
                            }
                            let past_host = has_host;
                            // Warped clips are time-stretched as they play; the rest, and clips
                            // the stretcher cannot handle, are re-pitched.
                            let voice_began = std::time::Instant::now();
                            let voice = (region.warped && region.stretch != suhina_model::StretchMode::Repitch)
                                .then(|| StretchVoice::on_map(clip, region, data.clone(), &tempo, sample_rate))
                                .flatten();
                            voices_took += voice_began.elapsed();
                            if let Some(voice) = voice {
                                let gain = if region.gain.is_finite() { region.gain.max(0.0) as f32 } else { 0.0 };
                                let to_frame = |beats: f64| tempo::frame_of(&tempo, beats);
                                let (start, end) = (to_frame(clip.start), to_frame(clip.start + clip.length));
                                let length = end.saturating_sub(start);
                                let key = ahead::voice_key(clip, region, data, (start, length), tempo_key);
                                stretch_voices.push((voice, start, length, key));
                                stretch_heard.push(track_gains(project, track).is_some());
                                stretch_events.push(StretchEvent {
                                    start,
                                    end,
                                    gain_left: gains.0 * gain,
                                    gain_right: gains.1 * gain,
                                    fades: Fades::on_map(clip, &tempo, sample_rate),
                                    column,
                                    bus,
                                    meter,
                                    past_host,
                                });
                                continue;
                            }
                            let timing =
                                audio::Timing { frames_per_beat, tempo_bpm: bpm, sample_rate, map: changing.clone() };
                            audio_events.extend(AudioEvent::new(clip, region, data.clone(), timing, gains).map(
                                |event| ArrangedAudio {
                                    event,
                                    column,
                                    bus,
                                    meter,
                                    key: (lane_index, clip_index, content_index),
                                    past_host,
                                },
                            ));
                        }
                    }
                }
            }
        }
        events.sort_by_key(|e| e.start);
        audio_events.sort_by_key(|a| a.event.start);
        hosted.sort_by_key(|h: &HostedClip| h.start);

        let release = (RELEASE_SECONDS * f64::from(sample_rate)) as u64;
        let notes_end = events.iter().map(|e| e.end.saturating_add(release)).max().unwrap_or(0);
        let length = audio_events
            .iter()
            .map(|a| a.event.end)
            .chain(stretch_events.iter().map(|e: &StretchEvent| e.end))
            .max()
            .unwrap_or(0)
            .max(notes_end);
        // A passthrough chain (a cued track's, or one into a bus) adds no tail of its own.
        let tail = if chains.iter().all(|c| !c.has_devices()) {
            0
        } else {
            (CHAIN_TAIL_SECONDS * f64::from(sample_rate)) as u64
        };
        let length = if length > 0 { length + tail + u64::from(latency) } else { 0 };
        let seconds = length as f64 / f64::from(sample_rate);
        if seconds > MAX_RENDER_SECONDS {
            return Err(RenderError::TooLong { seconds });
        }

        let inputs: Vec<input::InputRoute> = project
            .tracks
            .iter()
            .filter_map(|t| {
                let (left, right) = match t.audio_input.channels {
                    suhina_model::InputChannels::None => return None,
                    suhina_model::InputChannels::Mono(c) => (usize::from(c.checked_sub(1)?), usize::from(c - 1)),
                    suhina_model::InputChannels::Stereo(c) => (usize::from(c.checked_sub(1)?), usize::from(c)),
                };
                let (bus, (gain_left, gain_right)) = route(project, &chains, t.id)?;
                let gains = (gain_left * AUDIO_HEADROOM, gain_right * AUDIO_HEADROOM);
                Some(input::InputRoute { track: t.id, left, right, monitor: t.audio_input.monitor, bus, gains })
            })
            .collect();
        let rings_began = std::time::Instant::now();
        let voices = stretch_voices.len();
        let mut ahead = AheadReader::new(stretch_voices, sample_rate);
        ahead.set_heard(&stretch_heard);
        let rings_took = rings_began.elapsed();
        let launcher_began = std::time::Instant::now();
        let launcher = Launcher::new(project, audio, sample_rate, &chains, Arc::clone(&tempo));
        let launcher_took = launcher_began.elapsed();
        let levels_began = std::time::Instant::now();
        let levels = meters::Levels::build(project, &chains);
        let levels_took = levels_began.elapsed();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        build_report::set(format!(
            "graph {:.1} ms: before the chains {:.1} ms, device chains {:.1} ms, {voices} stretch voices {:.1} ms, their rings {:.1} ms, launcher {:.1} ms, levels {:.1} ms",
            ms(build_began.elapsed()),
            ms(chains_began.duration_since(build_began)),
            ms(chains_took),
            ms(voices_took),
            ms(rings_took),
            ms(launcher_took),
            ms(levels_took),
        ));
        Ok(Box::new(Graph {
            inputs_heard: vec![false; inputs.len()],
            inputs,
            events,
            audio_events,
            stretch_events,
            hosted,
            ahead,
            length,
            launcher,
            tempo,
            loop_beats: project.transport.looping().map(|r| (r.start, r.end)),
            routes: project
                .tracks
                .iter()
                .map(|t| {
                    let (bus, gains) = route(project, &chains, t.id).unwrap_or((0, (0.0, 0.0)));
                    (t.id, bus, gains)
                })
                .collect(),
            levels,
            chains,
            capture,
            latency,
            direct_delay: Delay::new(latency as usize),
        }))
    }
}

/// The channel `track`'s sounds play through, or the id the validation keeps out of reach: a
/// track without a channel has none.
fn node_channel(project: &Project, track: TrackId) -> ChannelId {
    project.track(track).and_then(|t| t.channel).unwrap_or(ChannelId(u32::MAX))
}

/// The project for a source render: the channels above the source — its groups and the master
/// — at unity and unmuted, so nothing after the node is in the render. The node's own channel
/// and every channel under it keep their volume, pan and mute. Allocates.
fn source_rest(project: &Project, source: TrackId) -> Project {
    let mut rest = project.clone();
    let Some(own) = project.track(source).and_then(|t| t.channel) else {
        return rest;
    };
    for id in project.output_chain(own) {
        if let Some(channel) = rest.channel_mut(id) {
            channel.volume = 1.0;
            channel.mute = false;
        }
    }
    // A channel with no output plays into the master without naming it: the master is above
    // every node too, so its fader and mute are not in the render either. The source's own
    // channel keeps its fader.
    for channel in rest.channels.iter_mut().filter(|c| c.role == ChannelRole::Master && c.id != own) {
        channel.volume = 1.0;
        channel.mute = false;
    }
    rest
}
