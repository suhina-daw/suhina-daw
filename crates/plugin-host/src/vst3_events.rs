// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notes and controllers for a VST3 instrument. Notes go to `process` as an `IEventList`;
//! controllers, pitch bend and channel pressure (aftertouch, #812) are not events in VST3 but
//! parameters, found through the controller's `IMidiMapping`, so they go as parameter changes
//! ([`crate::vst3_params`]). Both lists are made before processing starts, so the audio thread
//! does not allocate.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
pub(crate) struct MidiMapping {
    table: Arc<[Option<ParamID>; CONTROLLERS]>,
    shown: Shown,
}

/// No value sent since the main thread last looked: not the bits of any value in 0 to 1.
const UNCHANGED: u64 = u64::MAX;

/// The latest value the audio thread gave each mapped controller, for the main thread to give
/// the edit controller too ([`Shown::take`]): VST3 leaves keeping the controller in step with
/// the processor to the host, and the editor draws what the controller holds, so without it a
/// plug-in's on-screen wheels stay put while the sound follows a MIDI controller's. Set on the
/// audio thread and taken on the main thread without a lock.
#[derive(Clone)]
pub(crate) struct Shown {
    table: Arc<[Option<ParamID>; CONTROLLERS]>,
    values: Arc<[AtomicU64; CONTROLLERS]>,
    /// Each controller's slot in `values`: the first controller mapped to the same parameter,
    /// so two controllers moving one parameter keep one latest value, the last one sent.
    slot_of: Arc<[usize; CONTROLLERS]>,
}

impl Shown {
    /// Notes `value` as the latest of controller `number`'s parameter. Real-time safe.
    fn set(&self, number: usize, value: ParamValue) {
        let at = self.slot_of.get(number).copied().unwrap_or(number);
        if let Some(slot) = self.values.get(at) {
            slot.store(value.to_bits(), Ordering::Release);
        }
    }

    /// Notes `value` as the latest of parameter `id` when a controller is mapped to it: a
    /// parameter event after a controller's in the same block is what the processor ends at, so
    /// the editor is given it, not the controller's older value (#886). Real-time safe.
    fn set_param(&self, id: ParamID, value: ParamValue) {
        if let Some(number) = self.table.iter().position(|mapped| *mapped == Some(id)) {
            self.set(number, value);
        }
    }

    /// The parameters whose controller moved since the last call, each with its latest value.
    pub(crate) fn take(&self) -> impl Iterator<Item = (ParamID, ParamValue)> + '_ {
        self.values.iter().zip(self.table.iter()).filter_map(|(slot, id)| {
            let bits = slot.swap(UNCHANGED, Ordering::AcqRel);
            if bits == UNCHANGED {
                return None;
            }
            (*id).map(|id| (id, f64::from_bits(bits)))
        })
    }
}

impl MidiMapping {
    /// Asks `controller` for its assignments; a plug-in without a mapping maps nothing.
    pub(crate) fn of(controller: Option<&ComPtr<IEditController>>) -> Self {
        let mut table = Box::new([None; CONTROLLERS]);
        let Some(mapping) = controller.and_then(|c| c.cast::<IMidiMapping>()) else {
            // Without it a VST3 plug-in cannot be sent controllers or pitch bend at all.
            suhina_plugins::trace!(
                "vst3 midi mapping: {}",
                if controller.is_some() { "the controller has no IMidiMapping" } else { "no controller" }
            );
            return Self::with(table);
        };
        for (number, slot) in table.iter_mut().enumerate() {
            let mut id: ParamID = 0;
            // SAFETY: a live controller writing one id.
            let found = unsafe { mapping.getMidiControllerAssignment(0, 0, number as CtrlNumber, &mut id) };
            if found == kResultOk {
                *slot = Some(id);
            }
        }
        let mapped = table.iter().filter(|s| s.is_some()).count();
        let at = |number: usize| table.get(number).copied().flatten();
        suhina_plugins::trace!(
            "vst3 midi mapping: {mapped} of {CONTROLLERS} mapped; mod wheel {:?}, aftertouch {:?}, pitch bend {:?}",
            at(1),
            at(128),
            at(129)
        );
        Self::with(table)
    }

    fn with(table: Box<[Option<ParamID>; CONTROLLERS]>) -> Self {
        let table: Arc<[Option<ParamID>; CONTROLLERS]> = Arc::from(table);
        let values = Arc::new(std::array::from_fn(|_| AtomicU64::new(UNCHANGED)));
        let slot_of = Arc::new(std::array::from_fn(|number| match table.get(number).copied().flatten() {
            Some(id) => table.iter().position(|other| *other == Some(id)).unwrap_or(number),
            None => number,
        }));
        Self { shown: Shown { table: Arc::clone(&table), values, slot_of }, table }
    }

    /// Where the main thread finds the values it sent, to show them in the editor.
    pub(crate) fn shown(&self) -> Shown {
        self.shown.clone()
    }

    /// The parameter change a controller, pitch bend, channel pressure or parameter event
    /// makes: id, frame, normalized value. A controller's is noted for the editor too
    /// ([`Shown`]), and so is a parameter event's for a parameter a controller moves, so the
    /// editor ends at the value the processor does. Real-time safe.
    pub(crate) fn change(&self, event: &LinkEvent) -> Option<(ParamID, i32, ParamValue)> {
        if let EventKind::Param { id, value } = event.kind {
            // A parameter's own value, kept within VST3's normalized range.
            let value = f64::from(value).clamp(0.0, 1.0);
            self.shown.set_param(id, value);
            return Some((id, event.frame as i32, value));
        }
        let (number, id, value) = self.controller(event)?;
        self.shown.set(number, value);
        Some((id, event.frame as i32, value))
    }

    /// The controller number a controller, pitch bend or channel pressure event moves, the
    /// parameter mapped to it and its normalized value; `None` for anything else or a
    /// controller nothing is mapped to.
    pub(crate) fn controller(&self, event: &LinkEvent) -> Option<(usize, ParamID, ParamValue)> {
        let (number, value) = match event.kind {
            EventKind::Controller { number, value, .. } => (usize::from(number), f64::from(value)),
            // -1 to 1 becomes 0 to 1, the centre at one half.
            EventKind::PitchBend { value, .. } => (129, (f64::from(value) + 1.0) / 2.0),
            // Channel aftertouch, as VST3 numbers it (#812).
            EventKind::Pressure { value, .. } => (128, f64::from(value)),
            EventKind::Param { .. }
            | EventKind::NoteOn { .. }
            | EventKind::NoteOff { .. }
            | EventKind::Transport { .. }
            | EventKind::Tempo { .. }
            | EventKind::Beat { .. }
            | EventKind::Bar { .. }
            | EventKind::Cycle { .. }
            | EventKind::CycleSeconds { .. } => return None,
        };
        let id = (*self.table.get(number)?)?;
        Some((number, id, value.clamp(0.0, 1.0)))
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
        | EventKind::Pressure { .. }
        | EventKind::Param { .. }
        | EventKind::Transport { .. }
        | EventKind::Tempo { .. }
        | EventKind::Beat { .. }
        | EventKind::Bar { .. }
        | EventKind::Cycle { .. }
        | EventKind::CycleSeconds { .. } => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: EventKind) -> LinkEvent {
        LinkEvent { frame: 0, note: -1, kind }
    }

    #[test]
    fn two_controllers_on_one_parameter_show_the_last_value_sent() {
        // Aftertouch (128) and the volume controller (7) both move parameter 5.
        let mut table = Box::new([None; CONTROLLERS]);
        table[7] = Some(5);
        table[128] = Some(5);
        table[1] = Some(9);
        let mapping = MidiMapping::with(table);
        let shown = mapping.shown();
        // Pressure first, then CC7: the editor ends at CC7's value, as the processor does.
        mapping.change(&event(EventKind::Pressure { channel: 0, value: 0.2 }));
        mapping.change(&event(EventKind::Controller { channel: 0, number: 7, value: 0.9 }));
        mapping.change(&event(EventKind::Controller { channel: 0, number: 1, value: 0.5 }));
        let taken: Vec<(ParamID, ParamValue)> = shown.take().collect();
        assert_eq!(taken.len(), 2, "{taken:?}");
        assert!(taken.contains(&(5, f64::from(0.9f32))), "{taken:?}");
        assert!(taken.contains(&(9, 0.5)), "{taken:?}");
        // Taken once: nothing is left for the next look.
        assert_eq!(shown.take().count(), 0);
    }

    #[test]
    fn a_parameter_event_after_a_controller_in_one_block_is_the_value_shown() {
        // The mod wheel (1) moves parameter 9; automation then sets 9 later in the same block.
        let mut table = Box::new([None; CONTROLLERS]);
        table[1] = Some(9);
        let mapping = MidiMapping::with(table);
        let shown = mapping.shown();
        mapping.change(&event(EventKind::Controller { channel: 0, number: 1, value: 0.5 }));
        let last = mapping.change(&event(EventKind::Param { id: 9, value: 0.25 }));
        assert_eq!(last.map(|(id, _, value)| (id, value)), Some((9, 0.25)), "what the processor gets");
        assert_eq!(shown.take().collect::<Vec<_>>(), [(9, 0.25)], "the editor gets the same");
        // A parameter no controller moves is not the mapping's to show.
        mapping.change(&event(EventKind::Param { id: 4, value: 0.75 }));
        assert_eq!(shown.take().count(), 0);
    }
}
