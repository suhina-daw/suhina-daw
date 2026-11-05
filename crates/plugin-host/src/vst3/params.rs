// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parameter changes for a VST3 processor. A processor learns new parameter values only through
//! `process`: each block carries an `IParameterChanges` list with a queue of values per
//! parameter. The control thread puts `(id, value)` pairs in a [`Pending`] list; the audio
//! thread moves them into a list made before processing starts, so it does not allocate.

use std::cell::Cell;
use std::sync::{Arc, Mutex};

use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper};

use crate::param_notes::Notes;

/// Most parameter changes one block carries; more wait for the next block.
pub(crate) const MAX_CHANGES: usize = 4096;

/// Values waiting for the next block: parameter id and normalized value.
pub(crate) type Pending = Arc<Mutex<Vec<(ParamID, ParamValue)>>>;

/// Most values one parameter takes in one block; past that the last one is replaced.
const MAX_POINTS: usize = 16;

/// One parameter's changes in a block: up to [`MAX_POINTS`] values, each at an offset, in
/// order.
struct Queue {
    id: Cell<ParamID>,
    points: [Cell<(int32, ParamValue)>; MAX_POINTS],
    count: Cell<usize>,
}

impl Queue {
    fn new() -> Self {
        Self { id: Cell::new(0), points: std::array::from_fn(|_| Cell::new((0, 0.0))), count: Cell::new(0) }
    }

    /// Starts the queue over for parameter `id`.
    fn reset(&self, id: ParamID) {
        self.id.set(id);
        self.count.set(0);
    }

    /// Adds `value` at `offset`, never before the last point (points must be in order).
    fn push(&self, offset: int32, value: ParamValue) {
        let count = self.count.get();
        let last = count.checked_sub(1).and_then(|i| self.points.get(i)).map(|p| p.get().0);
        let offset = last.map_or(offset, |last| offset.max(last));
        if last == Some(offset)
            && let Some(point) = count.checked_sub(1).and_then(|i| self.points.get(i))
        {
            // A second value at the same frame replaces the first.
            point.set((offset, value));
            return;
        }
        let index = count.min(MAX_POINTS - 1);
        if let Some(point) = self.points.get(index) {
            point.set((offset, value));
        }
        self.count.set((count + 1).min(MAX_POINTS));
    }
}

impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id.get()
    }

    unsafe fn getPointCount(&self) -> int32 {
        self.count.get() as int32
    }

    unsafe fn getPoint(&self, index: int32, offset: *mut int32, value: *mut ParamValue) -> tresult {
        let point = usize::try_from(index).ok().filter(|i| *i < self.count.get()).and_then(|i| self.points.get(i));
        let Some(point) = point.filter(|_| !offset.is_null() && !value.is_null()) else { return kInvalidArgument };
        let (at, to) = point.get();
        // SAFETY: both pointers are non-null and point at values the plug-in passed to fill.
        unsafe { *offset = at };
        // SAFETY: as above.
        unsafe { *value = to };
        kResultOk
    }

    unsafe fn addPoint(&self, _offset: int32, _value: ParamValue, _index: *mut int32) -> tresult {
        kResultFalse
    }
}

/// The changes of one block.
struct Changes {
    queues: Vec<ComWrapper<Queue>>,
    pointers: Vec<ComPtr<IParamValueQueue>>,
    count: Cell<usize>,
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> int32 {
        self.count.get() as int32
    }

    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        match usize::try_from(index).ok().filter(|i| *i < self.count.get()).and_then(|i| self.pointers.get(i)) {
            // The list keeps its reference; the plug-in borrows it for the block.
            Some(queue) => queue.as_ptr(),
            None => std::ptr::null_mut(),
        }
    }

    unsafe fn addParameterData(&self, _id: *const ParamID, _index: *mut int32) -> *mut IParamValueQueue {
        std::ptr::null_mut()
    }
}

/// The audio thread's side: the list it hands to `process`.
pub(crate) struct BlockChanges {
    changes: ComWrapper<Changes>,
    pointer: Option<ComPtr<IParameterChanges>>,
    pending: Pending,
    /// What the editor changed and a block took in, for the main thread to report to the
    /// session (which keeps the project's values in step with it).
    applied: Notes,
    /// What the engine sent with a block (a panel set, automation): the last value each
    /// parameter got, for the main thread to give the controller too, so the plug-in's editor
    /// follows the panel and automation.
    engine: Notes,
}

impl BlockChanges {
    /// Made before processing starts; allocates the list for [`MAX_CHANGES`] parameters.
    pub(crate) fn new(pending: Pending, applied: Notes, engine: Notes) -> Self {
        let queues: Vec<ComWrapper<Queue>> = (0..MAX_CHANGES).map(|_| ComWrapper::new(Queue::new())).collect();
        let pointers = queues.iter().filter_map(|q| q.to_com_ptr::<IParamValueQueue>()).collect();
        let changes = ComWrapper::new(Changes { queues, pointers, count: Cell::new(0) });
        let pointer = changes.to_com_ptr::<IParameterChanges>();
        Self { changes, pointer, pending, applied, engine }
    }

    /// Takes what is pending (if the control thread is not using the list right now) and the
    /// block's `extra` changes (id, frame, value, in frame order: a controller moving,
    /// automation), and returns the list for this block, or null when nothing changed. A
    /// parameter changed several times gets each value at its frame. Real-time safe: no
    /// allocation, and it never waits for a lock.
    pub(crate) fn take(&mut self, extra: impl Iterator<Item = (ParamID, int32, ParamValue)>) -> *mut IParameterChanges {
        let mut count = 0;
        let mut add = |id: ParamID, offset: int32, value: ParamValue| {
            let queues = &self.changes.queues;
            let found = queues.iter().take(count).position(|q| q.id.get() == id);
            let slot = found.or_else(|| {
                (count < queues.len()).then(|| {
                    count += 1;
                    count - 1
                })
            });
            if let Some(queue) = slot.and_then(|i| queues.get(i)) {
                if found.is_none() {
                    queue.reset(id);
                }
                queue.push(offset, value);
            }
        };
        if let Ok(mut pending) = self.pending.try_lock() {
            let room = pending.len().min(MAX_CHANGES);
            for (id, value) in pending.drain(..room) {
                add(id, 0, value);
                // Past the room the report loses the rest; the next edit reports again.
                self.applied.note(id, value);
            }
        }
        for (id, offset, value) in extra {
            add(id, offset, value);
        }
        // The last value each parameter took in this block, for the controller to show.
        for queue in self.changes.queues.iter().take(count) {
            let id = queue.id.get();
            let last = (0..queue.count.get()).next_back().and_then(|i| queue.points.get(i)).map(|p| p.get().1);
            if let Some(value) = last {
                self.engine.note(id, value);
            }
        }
        self.changes.count.set(count);
        if count == 0 {
            std::ptr::null_mut()
        } else {
            self.pointer.as_ref().map_or(std::ptr::null_mut(), ComPtr::as_ptr)
        }
    }

    /// Empties the list after the block.
    pub(crate) fn done(&self) {
        self.changes.count.set(0);
    }
}

#[cfg(test)]
mod tests {
    use vst3::ComRef;

    use super::*;
    use crate::param_notes::ParamNotes;

    #[test]
    fn a_parameter_changed_several_times_in_a_block_gets_each_value() {
        let pending = Pending::default();
        if let Ok(mut list) = pending.lock() {
            list.push((5, 0.1));
        }
        let applied = Notes::new(ParamNotes::new());
        let engine = Notes::new(ParamNotes::new());
        let mut applied_read = ParamNotes::fresh_reads();
        let mut engine_read = ParamNotes::fresh_reads();
        let mut changes = BlockChanges::new(pending, applied.clone(), engine.clone());
        // In frame order; a second value at frame 20 replaces the first, and one before the
        // last point is moved to it.
        let extra = [(5, 10, 0.2), (5, 20, 0.4), (5, 20, 0.5), (7, 3, 0.9), (5, 15, 0.6)];
        let list = changes.take(extra.into_iter());
        // SAFETY: the list `take` returned, alive while `changes` is.
        let list = unsafe { ComRef::from_raw(list) }.unwrap();
        // SAFETY: a live list.
        assert_eq!(unsafe { list.getParameterCount() }, 2);
        // SAFETY: as above; index 0 is below its count.
        let raw = unsafe { list.getParameterData(0) };
        // SAFETY: a queue the list holds while `changes` is alive.
        let queue = unsafe { ComRef::from_raw(raw) }.unwrap();
        // SAFETY: a live queue.
        assert_eq!((unsafe { queue.getParameterId() }, unsafe { queue.getPointCount() }), (5, 3));
        let points: Vec<(i32, f64)> = (0..3)
            .map(|index| {
                let (mut offset, mut value) = (0, 0.0);
                // SAFETY: a live queue and two values to fill.
                assert_eq!(unsafe { queue.getPoint(index, &mut offset, &mut value) }, kResultOk);
                (offset, value)
            })
            .collect();
        assert_eq!(points, [(0, 0.1), (10, 0.2), (20, 0.6)]);
        changes.done();
        // The next block starts empty.
        assert!(changes.take(std::iter::empty()).is_null());
        // The editor's edit was reported and the engine's last value recorded.
        assert_eq!(applied.take(&mut applied_read), [(5, 0.1)]);
        assert_eq!(engine.take(&mut engine_read).into_iter().find(|(id, _)| *id == 5), Some((5, 0.6)));
    }
}
