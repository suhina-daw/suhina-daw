// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! VST3 ABI for the shared native adapter. Raw host pointers are confined to this module.

use super::automation::Automation;
use super::{Event as NativeEvent, EventKind, MAX_FRAMES, Parameters, Processor, state};
use std::cell::RefCell;
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};
use suhina_device_api::{DeviceInfo, ParamKind, ProcessContext as NativeContext, Role};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComRef, ComWrapper, uid};

mod controller;
mod factory;
#[cfg(test)]
mod tests;

const MAX_EVENTS: usize = 4096;

fn class_id(id: &str, controller: bool) -> TUID {
    fn hash(id: &str, seed: u32) -> u32 {
        id.bytes().fold(seed, |h, b| (h ^ u32::from(b)).wrapping_mul(16_777_619))
    }
    uid(0x5355_4849, if controller { 0x4354_524c } else { 0x4453_5021 }, hash(id, 2_166_136_261), hash(id, 0x9e37_79b9))
}

fn text(source: &str, destination: &mut [c_char]) {
    destination.fill(0);
    let room = destination.len().saturating_sub(1);
    for (to, from) in destination.iter_mut().take(room).zip(source.bytes()) {
        *to = from as c_char;
    }
}
fn wide(source: &str, destination: &mut [TChar]) {
    destination.fill(0);
    let room = destination.len().saturating_sub(1);
    for (to, from) in destination.iter_mut().take(room).zip(source.encode_utf16()) {
        *to = from as TChar;
    }
}

struct Component {
    parameters: Parameters,
    runtime: RefCell<Option<Runtime>>,
    latency: AtomicU32,
}
struct Runtime {
    processor: Processor,
    buffer: Vec<f32>,
    ordered: Vec<(usize, NativeEvent)>,
    events: Vec<NativeEvent>,
    automation: Automation,
    seen_parameters: Vec<bool>,
}
impl Class for Component {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}
impl IPluginBaseTrait for Component {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl Component {
    fn input_count(&self) -> i32 {
        i32::from(self.parameters.info.role != Role::Instrument)
    }
    fn bus_count(&self, media: MediaType, direction: BusDirection) -> i32 {
        if direction != BusDirections_::kInput as BusDirection && direction != BusDirections_::kOutput as BusDirection {
            return 0;
        }
        if media == MediaTypes_::kAudio as MediaType {
            if direction == BusDirections_::kOutput as BusDirection { 1 } else { self.input_count() }
        } else if media == MediaTypes_::kEvent as MediaType {
            i32::from(
                direction == BusDirections_::kInput as BusDirection && self.parameters.info.role == Role::Instrument,
            )
        } else {
            0
        }
    }
}

impl IComponentTrait for Component {
    unsafe fn getControllerClassId(&self, id: *mut TUID) -> tresult {
        // SAFETY: the host supplies a writable class id or null.
        let Some(id) = (unsafe { id.as_mut() }) else { return kInvalidArgument };
        *id = class_id(&self.parameters.info.id, true);
        kResultOk
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, direction: BusDirection) -> i32 {
        self.bus_count(media, direction)
    }
    unsafe fn getBusInfo(&self, media: MediaType, direction: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        if index < 0 || index >= self.bus_count(media, direction) {
            return kInvalidArgument;
        }
        // SAFETY: the host supplies one writable BusInfo or null.
        let Some(bus) = (unsafe { bus.as_mut() }) else { return kInvalidArgument };
        bus.mediaType = media;
        bus.direction = direction;
        bus.channelCount = if media == MediaTypes_::kAudio as MediaType { 2 } else { 16 };
        bus.busType = BusTypes_::kMain as BusType;
        bus.flags = BusInfo_::BusFlags_::kDefaultActive as uint32;
        wide(if media == MediaTypes_::kEvent as MediaType { "Notes" } else { "Main" }, &mut bus.name);
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _input: *mut RoutingInfo, _output: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(&self, media: MediaType, direction: BusDirection, index: i32, _active: TBool) -> tresult {
        if index >= 0 && index < self.bus_count(media, direction) { kResultOk } else { kInvalidArgument }
    }
    unsafe fn setActive(&self, active: TBool) -> tresult {
        let Ok(mut runtime) = self.runtime.try_borrow_mut() else { return kResultFalse };
        if active != 0
            && let Some(runtime) = runtime.as_mut()
        {
            runtime.processor.sync_parameters(&self.parameters);
            runtime.processor.reset();
        }
        kResultOk
    }
    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: the host supplies a live stream interface for this call or null.
        unsafe { read_state(&self.parameters, stream) }
    }
    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        // SAFETY: the host supplies a live stream interface for this call or null.
        unsafe { write_state(&self.parameters, stream) }
    }
}

impl IAudioProcessorTrait for Component {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut SpeakerArrangement,
        ins: i32,
        outputs: *mut SpeakerArrangement,
        outs: i32,
    ) -> tresult {
        if ins != self.input_count() || outs != 1 {
            return kResultFalse;
        }
        // SAFETY: with one advertised output the host supplies its arrangement or null.
        if unsafe { outputs.as_ref() } != Some(&SpeakerArr::kStereo) {
            return kResultFalse;
        }
        // SAFETY: the host supplies the one input arrangement when ins is one.
        if ins == 1 && unsafe { inputs.as_ref() } != Some(&SpeakerArr::kStereo) {
            return kResultFalse;
        }
        kResultOk
    }
    unsafe fn getBusArrangement(
        &self,
        direction: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        if index != 0 || self.bus_count(MediaTypes_::kAudio as MediaType, direction) == 0 {
            return kInvalidArgument;
        }
        // SAFETY: the host supplies a writable arrangement or null.
        let Some(arrangement) = (unsafe { arrangement.as_mut() }) else { return kInvalidArgument };
        *arrangement = SpeakerArr::kStereo;
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as int32 { kResultOk } else { kResultFalse }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        self.latency.load(Ordering::Relaxed)
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        // SAFETY: the host supplies a live setup for this call or null.
        let Some(setup) = (unsafe { setup.as_ref() }) else { return kInvalidArgument };
        if setup.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as int32 || setup.maxSamplesPerBlock <= 0 {
            return kInvalidArgument;
        }
        let frames = setup.maxSamplesPerBlock as usize;
        let Ok(processor) = Processor::new(&self.parameters, setup.sampleRate, frames) else { return kInvalidArgument };
        self.latency.store(processor.latency(), Ordering::Relaxed);
        let Ok(mut runtime) = self.runtime.try_borrow_mut() else { return kResultFalse };
        *runtime = Some(Runtime {
            processor,
            buffer: vec![0.0; frames * 2],
            ordered: Vec::with_capacity(MAX_EVENTS),
            events: Vec::with_capacity(MAX_EVENTS),
            automation: Automation::new(MAX_EVENTS),
            seen_parameters: vec![false; self.parameters.info.params.len()],
        });
        kResultOk
    }
    unsafe fn setProcessing(&self, _active: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        // SAFETY: the host supplies its ProcessData and its reachable buffers for this call.
        let Some(data) = (unsafe { data.as_mut() }) else { return kInvalidArgument };
        let Ok(mut runtime) = self.runtime.try_borrow_mut() else { return kResultFalse };
        let Some(runtime) = runtime.as_mut() else { return kResultFalse };
        // SAFETY: the validated host ProcessData is live; processing retains no pointers.
        let result = unsafe { runtime.process(&self.parameters, data, self.input_count()) };
        if result != kResultOk {
            runtime.processor.reset();
            // SAFETY: only validated bounded output buffers are cleared by this helper.
            unsafe { silence(data) };
        }
        result
    }
    unsafe fn getTailSamples(&self) -> u32 {
        u32::MAX
    }
}
impl IProcessContextRequirementsTrait for Component {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        (IProcessContextRequirements_::Flags_::kNeedTempo
            | IProcessContextRequirements_::Flags_::kNeedProjectTimeMusic
            | IProcessContextRequirements_::Flags_::kNeedTransportState) as uint32
    }
}

impl Runtime {
    unsafe fn process(&mut self, parameters: &Parameters, data: &mut ProcessData, inputs: i32) -> tresult {
        if data.numSamples < 0
            || data.numSamples as usize > MAX_FRAMES
            || data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as int32
        {
            return kInvalidArgument;
        }
        let frames = data.numSamples as usize;
        let Some(buffer) = self.buffer.get_mut(..frames * 2) else { return kInvalidArgument };
        self.ordered.clear();
        self.events.clear();
        self.automation.clear();
        self.seen_parameters.fill(false);
        let mut point_count = 0;
        // SAFETY: the host's optional parameter-change interface lives for process.
        let changes = unsafe { ComRef::from_raw(data.inputParameterChanges) };
        if let Some(changes) = changes {
            // SAFETY: changes is a live host interface.
            let count = unsafe { changes.getParameterCount() };
            if count < 0 || count as usize > super::MAX_PARAMS {
                return kInvalidArgument;
            }
            for index in 0..count {
                // SAFETY: index is within the host-reported bounded queue count.
                let raw = unsafe { changes.getParameterData(index) };
                // SAFETY: the queue belongs to changes and remains live for process.
                let Some(queue) = (unsafe { ComRef::from_raw(raw) }) else { continue };
                // SAFETY: queue is a live host interface.
                let id = unsafe { queue.getParameterId() };
                let parameter_index = parameters.index(id);
                let parameter = parameter_index.and_then(|index| parameters.info.params.get(index));
                if let Some(seen) = parameter_index.and_then(|index| self.seen_parameters.get_mut(index)) {
                    if *seen {
                        return kInvalidArgument;
                    }
                    *seen = true;
                }
                let continuous =
                    frames > 0 && parameter.is_some_and(|parameter| matches!(parameter.kind, ParamKind::Continuous(_)));
                if continuous
                    && let Some(index) = parameter_index
                    && self.automation.begin(index).is_err()
                {
                    return kInvalidArgument;
                }
                // SAFETY: queue is a live host interface.
                let count = unsafe { queue.getPointCount() };
                if count < 0 || count as usize > MAX_EVENTS - point_count {
                    return kInvalidArgument;
                }
                point_count += count as usize;
                let mut previous = None;
                for index in 0..count {
                    let (mut offset, mut value) = (0, 0.0);
                    // SAFETY: index is in the queue, output pointers are writable stack values.
                    if unsafe { queue.getPoint(index, &raw mut offset, &raw mut value) } != kResultOk
                        || offset < 0
                        || (frames == 0 && offset != 0)
                        || (frames > 0 && offset as usize >= frames)
                        || !value.is_finite()
                        || !(0.0..=1.0).contains(&value)
                        || previous.is_some_and(|previous| offset <= previous)
                    {
                        return kInvalidArgument;
                    }
                    previous = Some(offset);
                    if continuous {
                        if self.automation.push(offset as usize, value).is_err() {
                            return kInvalidArgument;
                        }
                    } else if let Some(parameter) = parameter {
                        let event = NativeEvent {
                            frame: offset as usize,
                            kind: EventKind::Parameter { id, value: parameter.from_normalized(value) },
                        };
                        self.ordered.push((self.ordered.len(), event));
                    }
                }
            }
        }
        // SAFETY: the host's optional event list lives for process.
        if let Some(events) = unsafe { ComRef::from_raw(data.inputEvents) } {
            // SAFETY: events is a live host interface.
            let count = unsafe { events.getEventCount() };
            if count < 0 || count as usize > MAX_EVENTS - point_count {
                return kInvalidArgument;
            }
            for index in 0..count {
                let mut event = std::mem::MaybeUninit::<Event>::uninit();
                // SAFETY: event is writable storage and index is within the reported count.
                if unsafe { events.getEvent(index, event.as_mut_ptr()) } != kResultOk {
                    return kInvalidArgument;
                }
                // SAFETY: a successful getEvent initialized the entire event.
                let event = unsafe { event.assume_init() };
                if event.sampleOffset < 0
                    || (frames == 0 && event.sampleOffset != 0)
                    || (frames > 0 && event.sampleOffset as usize >= frames)
                {
                    return kInvalidArgument;
                }
                let note = match event.r#type as Event_::EventTypes {
                    Event_::EventTypes_::kNoteOnEvent => {
                        // SAFETY: the discriminant selects the noteOn union member.
                        let note = unsafe { event.__field0.noteOn };
                        u8::try_from(note.pitch)
                            .ok()
                            .map(|key| suhina_device_api::NoteEvent::On { key, velocity: note.velocity })
                    }
                    Event_::EventTypes_::kNoteOffEvent => {
                        // SAFETY: the discriminant selects the noteOff union member.
                        let note = unsafe { event.__field0.noteOff };
                        u8::try_from(note.pitch)
                            .ok()
                            .map(|key| suhina_device_api::NoteEvent::Off { key, velocity: note.velocity })
                    }
                    _ => None,
                };
                if let Some(note) = note {
                    self.ordered.push((
                        self.ordered.len(),
                        NativeEvent { frame: event.sampleOffset as usize, kind: EventKind::Note(note) },
                    ));
                }
            }
        }
        self.ordered.sort_unstable_by_key(|(order, event)| (event.frame, *order));
        self.events.extend(self.ordered.iter().map(|(_, event)| *event));
        if frames == 0 {
            if self.events.iter().any(|event| event.frame != 0) {
                return kInvalidArgument;
            }
            self.processor.sync_parameters(parameters);
            for event in &self.events {
                self.processor.apply(parameters, event.kind);
            }
            return kResultOk;
        }
        if data.numInputs != inputs || data.numOutputs != 1 {
            return kInvalidArgument;
        }
        // SAFETY: the host declares one output bus; validate before touching DSP or parameters.
        let Some(output_channels) = (unsafe { channels(data.outputs) }) else { return kInvalidArgument };
        buffer.fill(0.0);
        if inputs == 1 {
            // SAFETY: the host declares exactly one input bus; helper validates stereo pointers.
            let Some(channels) = (unsafe { channels(data.inputs) }) else { return kInvalidArgument };
            for (channel, pointer) in channels.into_iter().enumerate() {
                // SAFETY: host channel has numSamples live f32 samples; copied before output writes.
                let input = unsafe { std::slice::from_raw_parts(pointer, frames) };
                for (frame, value) in buffer.as_chunks_mut::<2>().0.iter_mut().zip(input) {
                    if let Some(destination) = frame.get_mut(channel) {
                        *destination = *value;
                    }
                }
            }
        }
        // SAFETY: optional transport data lives for this process call.
        let context = unsafe { data.processContext.as_ref() }
            .map(|context| NativeContext {
                tempo: if context.state & ProcessContext_::StatesAndFlags_::kTempoValid as uint32 != 0 {
                    context.tempo
                } else {
                    120.0
                },
                beat: if context.state & ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid as uint32 != 0 {
                    context.projectTimeMusic
                } else {
                    0.0
                },
                playing: context.state & ProcessContext_::StatesAndFlags_::kPlaying as uint32 != 0,
            })
            .unwrap_or_default();
        if self.processor.process_automated(parameters, context, buffer, &self.events, &mut self.automation).is_err() {
            return kInvalidArgument;
        }
        for (channel, pointer) in output_channels.into_iter().enumerate() {
            // SAFETY: this output channel has numSamples writable f32 samples. No input borrow remains.
            let output = unsafe { std::slice::from_raw_parts_mut(pointer, frames) };
            for (value, frame) in output.iter_mut().zip(buffer.as_chunks::<2>().0) {
                *value = frame.get(channel).copied().unwrap_or(0.0);
            }
        }
        // SAFETY: one output bus was validated above and stays writable for process.
        if let Some(output) = unsafe { data.outputs.as_mut() } {
            output.silenceFlags = 0;
        }
        kResultOk
    }
}

/// Read a stereo channel-pointer pair without retaining a borrow of the host's bus.
unsafe fn channels(bus: *mut AudioBusBuffers) -> Option<[*mut f32; 2]> {
    // SAFETY: caller established that the pointer refers to the host's single bus or is null.
    let bus = unsafe { bus.as_ref() }?;
    if bus.numChannels != 2 {
        return None;
    }
    // SAFETY: callers require kSample32, selecting this union field.
    let pointers = unsafe { bus.__field0.channelBuffers32 };
    if pointers.is_null() {
        return None;
    }
    // SAFETY: the stereo bus contains exactly two channel pointers.
    let pair = unsafe { std::ptr::read(pointers.cast::<[*mut f32; 2]>()) };
    if pair.iter().any(|pointer| pointer.is_null()) { None } else { Some(pair) }
}

unsafe fn silence(data: &mut ProcessData) {
    if data.numSamples <= 0
        || data.numSamples as usize > MAX_FRAMES
        || data.numOutputs != 1
        || data.symbolicSampleSize != SymbolicSampleSizes_::kSample32 as int32
    {
        return;
    }
    // SAFETY: the bounded host block declares one output bus.
    if let Some(channels) = unsafe { channels(data.outputs) } {
        for pointer in channels {
            // SAFETY: each host channel is writable for the declared bounded sample count.
            unsafe { std::slice::from_raw_parts_mut(pointer, data.numSamples as usize) }.fill(0.0);
        }
    }
}

unsafe fn read_state(parameters: &Parameters, stream: *mut IBStream) -> tresult {
    // SAFETY: caller supplies a live host stream interface or null.
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else { return kInvalidArgument };
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        let mut read = 0;
        // SAFETY: chunk/read are writable; the stream does not retain them.
        let result = unsafe { stream.read(chunk.as_mut_ptr().cast(), chunk.len() as i32, &raw mut read) };
        if read < 0 || read as usize > chunk.len() || bytes.len() + read as usize > state::MAX_BYTES {
            return kResultFalse;
        }
        if let Some(part) = chunk.get(..read as usize) {
            bytes.extend_from_slice(part);
        }
        if read == 0 {
            break;
        }
        if result != kResultOk && result != kResultFalse {
            return kResultFalse;
        }
    }
    if state::load(parameters, &bytes).is_ok() { kResultOk } else { kResultFalse }
}
unsafe fn write_state(parameters: &Parameters, stream: *mut IBStream) -> tresult {
    // SAFETY: caller supplies a live host stream interface or null.
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else { return kInvalidArgument };
    let bytes = state::save(parameters);
    let mut position = 0;
    while let Some(remaining) = bytes.get(position..).filter(|remaining| !remaining.is_empty()) {
        let mut written = 0;
        // SAFETY: remaining is live for this write; IBStream reads this buffer and retains nothing.
        let result =
            unsafe { stream.write(remaining.as_ptr().cast_mut().cast(), remaining.len() as i32, &raw mut written) };
        if result != kResultOk || written <= 0 || written as usize > remaining.len() {
            return kResultFalse;
        }
        position += written as usize;
    }
    kResultOk
}
