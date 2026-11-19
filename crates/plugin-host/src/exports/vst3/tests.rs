// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use vst3::ComPtr;

struct Queue {
    id: u32,
    points: Vec<(i32, f64)>,
}
impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}
impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> u32 {
        self.id
    }
    unsafe fn getPointCount(&self) -> i32 {
        self.points.len() as i32
    }
    unsafe fn getPoint(&self, index: i32, offset: *mut i32, value: *mut f64) -> tresult {
        let (at, to) = self.points[index as usize];
        // SAFETY: the tested adapter supplies writable stack outputs.
        unsafe {
            offset.write(at);
        }
        // SAFETY: the tested adapter supplies a separate writable value output.
        unsafe {
            value.write(to);
        }
        kResultOk
    }
    unsafe fn addPoint(&self, _offset: i32, _value: f64, _index: *mut i32) -> tresult {
        kNotImplemented
    }
}
struct Changes {
    queues: Vec<ComPtr<IParamValueQueue>>,
}
impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}
impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> i32 {
        self.queues.len() as i32
    }
    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        self.queues[index as usize].as_ptr()
    }
    unsafe fn addParameterData(&self, _id: *const u32, _index: *mut i32) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

fn component() -> Component {
    let info = suhina_device_registry::DEVICES.iter().find(|info| info.id == "utility").unwrap();
    let component = Component {
        parameters: Parameters::new(info).unwrap(),
        runtime: RefCell::new(None),
        latency: AtomicU32::new(0),
    };
    let mut setup = ProcessSetup {
        processMode: ProcessModes_::kOffline as int32,
        symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
        maxSamplesPerBlock: 256,
        sampleRate: 48_000.0,
    };
    // SAFETY: setup points to initialized stack storage for this synchronous call.
    assert_eq!(unsafe { component.setupProcessing(&mut setup) }, kResultOk);
    component
}

fn block(component: &Component, queues: Vec<(usize, Vec<(i32, f64)>)>, frames: usize) -> (tresult, Vec<f32>) {
    let changes = ComWrapper::new(Changes {
        queues: queues
            .into_iter()
            .map(|(index, points)| {
                ComWrapper::new(Queue { id: component.parameters.id(index).unwrap(), points }).to_com_ptr().unwrap()
            })
            .collect(),
    })
    .to_com_ptr::<IParameterChanges>()
    .unwrap();
    let mut left = vec![0.25; frames];
    let mut right = vec![-0.125; frames];
    let mut pointers = [left.as_mut_ptr(), right.as_mut_ptr()];
    // SAFETY: these ABI structs contain only numeric fields and nullable raw pointers.
    let mut bus: AudioBusBuffers = unsafe { std::mem::zeroed() };
    bus.numChannels = 2;
    bus.silenceFlags = 3;
    bus.__field0.channelBuffers32 = pointers.as_mut_ptr();
    // SAFETY: as above; required nonzero fields and pointers are assigned below.
    let mut data: ProcessData = unsafe { std::mem::zeroed() };
    data.symbolicSampleSize = SymbolicSampleSizes_::kSample32 as int32;
    data.numSamples = frames as i32;
    data.numInputs = 1;
    data.numOutputs = 1;
    data.inputs = &mut bus;
    data.outputs = &mut bus;
    data.inputParameterChanges = changes.as_ptr();
    // SAFETY: buses, queue COM objects and channel storage outlive the synchronous call.
    let result = unsafe { component.process(&mut data) };
    if result == kResultOk && frames > 0 {
        assert_eq!(bus.silenceFlags, 0);
    }
    (result, left.into_iter().zip(right).flat_map(|(l, r)| [l, r]).collect())
}

fn ramp_render(partitions: &[usize]) -> Vec<f32> {
    let component = component();
    let gain = &component.parameters.info.params[0];
    // Publish after setup: the implicit -1 start must use this accepted state.
    component.parameters.restore(&[(&gain.id, gain.from_normalized(0.2))]).unwrap();
    let mut position = 0;
    let mut rendered = Vec::new();
    for &frames in partitions {
        position += frames;
        let end = 0.2 + 0.6 * position as f64 / 256.0;
        let (result, audio) = block(&component, vec![(0, vec![(frames as i32 - 1, end)])], frames);
        assert_eq!(result, kResultOk);
        rendered.extend(audio);
    }
    rendered
}

#[test]
fn normalized_ramps_match_stock_and_do_not_depend_on_host_block_partition() {
    let whole = ramp_render(&[256]);
    for partition in [&[32; 8][..], &[17, 53, 1, 185][..]] {
        let split = ramp_render(partition);
        assert!(whole.iter().zip(split).all(|(a, b)| (a - b).abs() < 1e-6));
    }
    let info = suhina_device_registry::DEVICES.iter().find(|info| info.id == "utility").unwrap();
    let mut stock = suhina_device_registry::create(&info.id, 48_000.0).unwrap();
    stock.set_param(0, info.params[0].from_normalized(0.2));
    stock.reset();
    let mut expected = Vec::new();
    for frame in 0..256 {
        stock.set_param(0, info.params[0].from_normalized(0.2 + 0.6 * (frame + 1) as f64 / 256.0));
        let mut audio = [0.25, -0.125];
        stock.process(&NativeContext::default(), &mut audio);
        expected.extend(audio);
    }
    assert_eq!(whole, expected, "interpolate normalized values before the nonlinear gain conversion");
}

#[test]
fn queues_hold_after_last_point_and_discrete_parameters_change_only_at_the_point() {
    let component = component();
    let (result, audio) = block(&component, vec![(0, vec![(0, 0.3), (7, 0.7)]), (11, vec![(12, 1.0)])], 32);
    assert_eq!(result, kResultOk);
    let info = component.parameters.info;
    let mut stock = suhina_device_registry::create(&info.id, 48_000.0).unwrap();
    stock.reset();
    let mut expected = Vec::new();
    for frame in 0..32 {
        let normalized = if frame <= 7 { 0.3 + 0.4 * frame as f64 / 7.0 } else { 0.7 };
        stock.set_param(0, info.params[0].from_normalized(normalized));
        if frame == 12 {
            stock.set_param(11, 1.0);
        }
        let mut sample = [0.25, -0.125];
        stock.process(&NativeContext::default(), &mut sample);
        expected.extend(sample);
    }
    assert!(audio.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-6));
}

#[test]
fn malformed_queues_do_not_partially_apply_state_or_parameters() {
    for bad in [
        vec![(0, 0.8), (32, 0.5)],
        vec![(0, 0.8), (1, f64::NAN)],
        vec![(2, 0.8), (1, 0.5)],
        vec![(0, 1.01)],
        vec![(-1, 0.5)],
    ] {
        let component = component();
        let before = state::save(&component.parameters);
        let (result, output) = block(&component, vec![(0, bad)], 32);
        assert_eq!(result, kInvalidArgument);
        assert!(output.iter().all(|sample| *sample == 0.0));
        assert_eq!(state::save(&component.parameters), before);
    }
    let component = component();
    let before = state::save(&component.parameters);
    assert_eq!(block(&component, vec![(0, vec![(0, 0.5)]), (1, vec![(1, 0.6)])], 0).0, kInvalidArgument);
    assert_eq!(state::save(&component.parameters), before);
}
