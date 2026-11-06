// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: Apache-2.0

/// An index identifies buffer memory, not a unique processing cycle.
#[derive(Default)]
pub(crate) struct CallbackCycle(Option<(i32, u64)>);

impl CallbackCycle {
    pub(crate) fn accept(&mut self, index: i32, timestamp: u64) -> bool {
        if self.0 == Some((index, timestamp)) {
            return false;
        }
        self.0 = Some((index, timestamp));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_buffer_driver_keeps_processing_new_cycles() {
        let mut cycle = CallbackCycle::default();
        for timestamp in [0, 46_439_909, 92_879_818, 139_319_727] {
            assert!(cycle.accept(0, timestamp));
            assert!(!cycle.accept(0, timestamp));
        }
    }

    #[test]
    fn alternating_buffers_and_clock_reset_are_accepted() {
        let mut cycle = CallbackCycle::default();
        assert!(cycle.accept(0, 100));
        assert!(cycle.accept(1, 100));
        assert!(!cycle.accept(1, 100));
        assert!(cycle.accept(1, 0));
        assert!(!cycle.accept(1, 0));
    }
}
