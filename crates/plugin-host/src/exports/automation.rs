// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded normalized control points, kept separate from discrete CLAP/native events.

use super::{Event, ExportError, Parameters, Processor};
use suhina_device_api::ProcessContext;

#[derive(Clone, Copy)]
pub(super) struct Point {
    pub frame: usize,
    pub value: f64,
}

struct Queue {
    parameter: usize,
    start: usize,
    end: usize,
    cursor: usize,
    from_frame: i32,
    from_value: f64,
}

/// Allocated at activation; queue and point counts cannot exceed their reserved capacities.
pub(super) struct Automation {
    points: Vec<Point>,
    queues: Vec<Queue>,
    limit: usize,
}

impl Automation {
    pub fn new(limit: usize) -> Self {
        Self { points: Vec::with_capacity(limit), queues: Vec::with_capacity(limit), limit }
    }

    pub fn clear(&mut self) {
        self.points.clear();
        self.queues.clear();
    }

    pub fn begin(&mut self, parameter: usize) -> Result<(), ExportError> {
        if self.queues.len() >= self.limit || self.queues.iter().any(|queue| queue.parameter == parameter) {
            return Err(ExportError::InvalidEvents);
        }
        self.queues.push(Queue {
            parameter,
            start: self.points.len(),
            end: self.points.len(),
            cursor: 0,
            from_frame: -1,
            from_value: 0.0,
        });
        Ok(())
    }

    pub fn push(&mut self, frame: usize, value: f64) -> Result<(), ExportError> {
        let Some(queue) = self.queues.last_mut() else { return Err(ExportError::InvalidEvents) };
        if self.points.len() >= self.limit
            || !value.is_finite()
            || !(0.0..=1.0).contains(&value)
            || (queue.end > queue.start && self.points.last().is_some_and(|point| point.frame >= frame))
        {
            return Err(ExportError::InvalidEvents);
        }
        self.points.push(Point { frame, value });
        queue.end = self.points.len();
        Ok(())
    }

    fn valid(&self, parameters: &Parameters, frames: usize) -> bool {
        self.points.iter().all(|point| point.frame < frames)
            && self.queues.iter().all(|queue| parameters.info.params.get(queue.parameter).is_some())
    }

    fn prepare(&mut self, processor: &Processor, parameters: &Parameters) {
        for queue in &mut self.queues {
            queue.cursor = queue.start;
            queue.from_frame = -1;
            // Read the coherently applied audio value, never a concurrently published state bank.
            queue.from_value = parameters
                .info
                .params
                .get(queue.parameter)
                .and_then(|parameter| {
                    processor.device.param(queue.parameter).map(|value| parameter.to_normalized(value))
                })
                .unwrap_or(0.0);
        }
    }

    fn apply(&mut self, frame: usize, processor: &mut Processor, parameters: &Parameters) {
        for queue in &mut self.queues {
            if queue.cursor >= queue.end {
                continue;
            }
            let Some(point) = self.points.get(queue.cursor) else { continue };
            let value = if frame == point.frame {
                queue.from_frame = frame as i32;
                queue.from_value = point.value;
                queue.cursor += 1;
                point.value
            } else {
                let fraction =
                    (frame as f64 - f64::from(queue.from_frame)) / (point.frame as f64 - f64::from(queue.from_frame));
                queue.from_value + (point.value - queue.from_value) * fraction
            };
            if let Some(parameter) = parameters.info.params.get(queue.parameter) {
                processor.apply_parameter(parameters, queue.parameter, parameter.from_normalized(value));
            }
        }
    }
}

impl Processor {
    /// VST3 ramps use the same native device and event semantics, synchronized once per host block.
    pub(super) fn process_automated(
        &mut self,
        parameters: &Parameters,
        context: ProcessContext,
        audio: &mut [f32],
        events: &[Event],
        automation: &mut Automation,
    ) -> Result<(), ExportError> {
        self.validate_block(audio, None, events)?;
        let frames = audio.len() / 2;
        if !automation.valid(parameters, frames) {
            audio.fill(0.0);
            return Err(ExportError::InvalidEvents);
        }
        if automation.points.is_empty() {
            return self.process(parameters, context, audio, None, events);
        }
        self.sync_parameters(parameters);
        automation.prepare(self, parameters);
        let mut events = events.iter().peekable();
        for frame in 0..frames {
            if automation.queues.iter().all(|queue| queue.cursor >= queue.end) {
                // Once ramps end, resume native spans while preserving later note/discrete offsets.
                let mut cursor = frame;
                for event in events {
                    self.span(context, cursor, event.frame, audio, None);
                    self.device.set_context(&self.context_at(context, event.frame));
                    self.apply(parameters, event.kind);
                    cursor = event.frame;
                }
                self.span(context, cursor, frames, audio, None);
                return Ok(());
            }
            self.device.set_context(&self.context_at(context, frame));
            automation.apply(frame, self, parameters);
            while events.peek().is_some_and(|event| event.frame == frame) {
                if let Some(event) = events.next() {
                    self.apply(parameters, event.kind);
                }
            }
            self.span(context, frame, frame + 1, audio, None);
        }
        Ok(())
    }
}
