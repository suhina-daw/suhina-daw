// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notes and controllers for a VST3 instrument. Notes go to `process` as an `IEventList`;
//! controllers and pitch bend are not events in VST3 but parameters, found through the
//! controller's `IMidiMapping`, so they go as parameter changes ([`crate::vst3_params`]). Both
//! lists are made before processing starts, so the audio thread does not allocate.

use std::cell::RefCell;

use suhina_ipc::{Event as LinkEvent, EventKind, MAX_EVENTS};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};

/// The controller numbers a mapping covers: MIDI controllers 0–127, then channel aftertouch
/// (128) and pitch bend (129), as VST3 numbers them.
const CONTROLLERS: usize = 130;

/// Which parameter MIDI controllers and pitch bend move, from the plug-in's `IMidiMapping`
/// (event bus 0). Only the first MIDI channel is asked, and its mapping serves every channel:
/// some plug-ins fail when asked about another channel, and the
/// notes a project plays come on the first.
pub(crate) struct MidiMapping(Box<[Option<ParamID>; CONTROLLERS]>);

impl MidiMapping {
    /// Asks `controller` for its assignments; a plug-in without a mapping maps nothing.
    pub(crate) fn of(controller: Option<&ComPtr<IEditController>>) -> Self {
        let mut table = Box::new([None; CONTROLLERS]);
        if let Some(mapping) = controller.and_then(|c| c.cast::<IMidiMapping>()) {
            for (number, slot) in table.iter_mut().enumerate() {
                let mut id: ParamID = 0;
                // SAFETY: a live controller writing one id.
                let found = unsafe { mapping.getMidiControllerAssignment(0, 0, number as CtrlNumber, &mut id) };
                if found == kResultOk {
                    *slot = Some(id);
                }
            }
        }
        Self(table)
    }

    /// The parameter change a controller, pitch bend or parameter event makes: id, frame,
    /// normalized value.
    pub(crate) fn change(&self, event: &LinkEvent) -> Option<(ParamID, i32, ParamValue)> {
        let (number, value) = match event.kind {
            EventKind::Controller { number, value, .. } => (usize::from(number), f64::from(value)),
            // -1 to 1 becomes 0 to 1, the centre at one half.
            EventKind::PitchBend { value, .. } => (129, (f64::from(value) + 1.0) / 2.0),
            // A parameter's own value, kept within VST3's normalized range.
            EventKind::Param { id, value } => return Some((id, event.frame as i32, f64::from(value).clamp(0.0, 1.0))),
            EventKind::NoteOn { .. }
            | EventKind::NoteOff { .. }
            | EventKind::Transport { .. }
            | EventKind::Tempo { .. } => return None,
        };
        let id = (*self.0.get(number)?)?;
        Some((id, event.frame as i32, value.clamp(0.0, 1.0)))
    }
}

/// One block's notes.
struct EventList {
    events: RefCell<Vec<Event>>,
}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        self.events.try_borrow().map_or(0, |e| e.len() as int32)
    }

    unsafe fn getEvent(&self, index: int32, event: *mut Event) -> tresult {
        let Ok(events) = self.events.try_borrow() else { return kResultFalse };
        let Some(found) = usize::try_from(index).ok().and_then(|i| events.get(i)) else { return kInvalidArgument };
        if event.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: the plug-in passes an event to fill; `found` is a plain C struct.
        unsafe { *event = *found };
        kResultOk
    }

    unsafe fn addEvent(&self, _event: *mut Event) -> tresult {
        kResultFalse
    }
}

/// The audio thread's side: the list handed to `process` as `inputEvents`.
pub(crate) struct BlockEvents {
    list: ComWrapper<EventList>,
    pointer: Option<ComPtr<IEventList>>,
}

impl BlockEvents {
    /// Made before processing starts, with room for [`MAX_EVENTS`] notes.
    pub(crate) fn new() -> Self {
        let list = ComWrapper::new(EventList { events: RefCell::new(Vec::with_capacity(MAX_EVENTS)) });
        let pointer = list.to_com_ptr::<IEventList>();
        Self { list, pointer }
    }

    /// The block's notes as VST3 events; null when there are none. Real-time safe.
    pub(crate) fn fill(&self, events: &[LinkEvent]) -> *mut IEventList {
        let Ok(mut list) = self.list.events.try_borrow_mut() else { return std::ptr::null_mut() };
        list.clear();
        for event in events.iter().take(MAX_EVENTS) {
            if let Some(converted) = note(event) {
                list.push(converted);
            }
        }
        if list.is_empty() {
            std::ptr::null_mut()
        } else {
            self.pointer.as_ref().map_or(std::ptr::null_mut(), ComPtr::as_ptr)
        }
    }
}

/// A note-on or note-off as a VST3 event on event bus 0.
fn note(event: &LinkEvent) -> Option<Event> {
    let mut out = Event {
        busIndex: 0,
        sampleOffset: event.frame as i32,
        ppqPosition: 0.0,
        flags: Event_::EventFlags_::kIsLive as uint16,
        r#type: 0,
        __field0: Event__type0 {
            noteOn: NoteOnEvent { channel: 0, pitch: 0, tuning: 0.0, velocity: 0.0, length: 0, noteId: -1 },
        },
    };
    match event.kind {
        EventKind::NoteOn { channel, key, velocity } => {
            out.r#type = Event_::EventTypes_::kNoteOnEvent as uint16;
            out.__field0 = Event__type0 {
                noteOn: NoteOnEvent {
                    channel: int16::from(channel),
                    pitch: int16::from(key),
                    tuning: 0.0,
                    velocity,
                    length: 0,
                    noteId: event.note,
                },
            };
        }
        EventKind::NoteOff { channel, key, velocity } => {
            out.r#type = Event_::EventTypes_::kNoteOffEvent as uint16;
            out.__field0 = Event__type0 {
                noteOff: NoteOffEvent {
                    channel: int16::from(channel),
                    pitch: int16::from(key),
                    velocity,
                    noteId: event.note,
                    tuning: 0.0,
                },
            };
        }
        EventKind::Controller { .. }
        | EventKind::PitchBend { .. }
        | EventKind::Param { .. }
        | EventKind::Transport { .. }
        | EventKind::Tempo { .. } => return None,
    }
    Some(out)
}
