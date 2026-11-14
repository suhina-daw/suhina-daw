// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one drag under way in the Arrangement (#1158): a clip, an edge, a fade, a header's
//! control, a marker, the loop, time being painted, a take, or on an automation lane a point, a
//! curve, a box or range, or lane headers. The view and its automation lanes share it. Only one runs at a time: a drag starts
//! through [`GestureSlot::begin`], which refuses while another is under way, and ends through one
//! of the `end_*` functions, which hand back what it carried for its command. Escape and a
//! release that the drag's widget never saw end it through [`GestureSlot::cancel`], which commits
//! nothing.

use egui::{Pos2, Vec2};
use suhina_engine::model::{ClipId, TrackId};

use crate::arrangement::arrangement_view::{
    ClipDrag, CurveDrag, EdgeDrag, FadeDrag, LoopGrab, TakeEdge, TakeMove, TimeSelection, ValueDrag,
};
use crate::arrangement::automation_lanes::{self as lanes, BoxDrag, HeaderDrag, NewPoint, PointDrag};
use crate::kit::widgets::Drag;

/// A selection rectangle or a time selection being painted from where the drag began.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Marquee {
    /// Where the drag began, on the screen as the lanes are scrolled now.
    pub(crate) origin: Pos2,
    /// The scroll when `origin` was last placed: the view scrolls under a drag past its edge,
    /// and the start moves with the lanes (#691).
    pub(crate) scroll: Vec2,
    /// It paints time (a lane's or a clip's upper half) rather than drawing a rectangle that
    /// picks whole clips (the lower half).
    pub(crate) time: bool,
    /// The stacked row a time-painting drag started in: its track, the row and how many.
    pub(crate) row: Option<(TrackId, usize, usize)>,
    /// What a drag with Shift goes on from, known once it has begun.
    pub(crate) kept: Option<Option<TimeSelection>>,
}

impl Marquee {
    pub(crate) fn new(origin: Pos2, scroll: Vec2, time: bool, row: Option<(TrackId, usize, usize)>) -> Self {
        Self { origin, scroll, time, row, kept: None }
    }
}

/// What the drag under way is doing, and what it carries so far.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Gesture {
    /// A drag over empty lanes or a clip's time-selecting part.
    Marquee(Marquee),
    /// A drag on the ruler selecting time over every track: the beat it began at, and what a drag
    /// with Shift goes on from.
    RulerSelect {
        from: f64,
        kept: Option<Option<TimeSelection>>,
    },
    /// The Draw tool on a lane: the track and the beat it started at.
    Draw((TrackId, f64)),
    Clip(ClipDrag),
    Edge(EdgeDrag),
    Fade(FadeDrag),
    FadeCurve(CurveDrag),
    /// A clip's gain, and its value so far.
    Gain(((TrackId, ClipId), f64)),
    /// A header's volume or pan.
    Value(ValueDrag),
    /// The main track's volume, and its value so far.
    Main(f64),
    /// A header's send knob: the track, the return and the level so far (dB).
    Send((TrackId, TrackId, f64)),
    /// A track header being moved to a new place.
    Track(TrackId),
    /// A marker: its index and where the pointer is, in beats before snapping.
    Marker((usize, f64)),
    /// The loop brace row: what it changes and where the pointer is, in beats before snapping.
    Loop((LoopGrab, f64)),
    /// Across a take lane, picking the part to comp: the track, the take's number and the beat it
    /// started at.
    Take((TrackId, usize, f64)),
    /// Ctrl and a drag painting an area over take lanes: the track, the take it started on and
    /// its first beat.
    TakeArea((TrackId, usize, f64)),
    /// Pieces of takes moved along the timeline (#1078).
    TakeMove(TakeMove),
    /// A lower corner of a piece of a take, moving that edge (#1078).
    TakeEdge(TakeEdge),
    /// The handle of a crossfade between parts of two takes: the part before the cut and the
    /// crossfade's length in beats so far (#1078).
    CompFade((ClipId, f64)),
    /// The bend of the crossfade after this part, and how far from its handle's middle the
    /// pointer went down (#1078).
    CompCurve((ClipId, f64, f32)),
    /// An automation point (with the rest of its selection), or a segment's two points with Shift.
    LanePoint(PointDrag),
    /// A press on an automation lane's line, which added a point there at once; once the point is
    /// on the lane, the drag holds it.
    NewPoint(NewPoint, Option<PointDrag>),
    /// An automation segment's curve being bent.
    LaneCurve(lanes::CurveDrag),
    /// A box selecting automation points, or a time range painted on a lane.
    LaneBox(BoxDrag),
    /// Automation lane headers being moved among their track's lanes (#1322).
    LaneHeaders(HeaderDrag),
}

/// The drag under way, if any (#1158).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GestureSlot {
    now: Option<Gesture>,
    /// Escape let go of a drag while its button is still held: the rest of that press starts no
    /// drag and does nothing on release, until every button is up.
    cancelled: bool,
}

impl GestureSlot {
    /// Whether a drag is under way.
    pub(crate) fn is_some(&self) -> bool {
        self.now.is_some()
    }

    /// The drag under way.
    pub(crate) fn get(&self) -> Option<&Gesture> {
        self.now.as_ref()
    }

    /// The drag under way, to change as the pointer moves.
    pub(crate) fn get_mut(&mut self) -> Option<&mut Gesture> {
        self.now.as_mut()
    }

    /// Starts `gesture`, when no drag is under way and no press Escape let go of is still held:
    /// the one place a drag starts. Returns whether it started; the drag under way is kept
    /// otherwise.
    pub(crate) fn begin(&mut self, gesture: Gesture) -> bool {
        if self.now.is_some() || self.cancelled {
            return false;
        }
        self.now = Some(gesture);
        true
    }

    /// Ends the drag under way when `mine` says it is the caller's, handing it back to commit:
    /// the one place a drag ends.
    pub(crate) fn end_if(&mut self, mine: impl FnOnce(&Gesture) -> bool) -> Option<Gesture> {
        self.now.take_if(|g| mine(g))
    }

    /// A value control's frame (a header's volume, pan or send, a clip's gain, the main volume):
    /// its drag begins as a gesture through [`Self::begin`] as `start` says, and the control's
    /// `Moving` frames and the `Done` of its release count only while that gesture is the drag
    /// under way, as `mine` says; after Escape, or while another drag runs, they are `None`. A
    /// release ends the gesture. A value set by the wheel, a key, typing or a reset is no drag
    /// and passes as it is.
    pub(crate) fn value_frame(
        &mut self,
        response: &egui::Response,
        drag: Drag,
        start: impl FnOnce() -> Gesture,
        mine: impl Fn(&Gesture) -> bool,
    ) -> Drag {
        if response.drag_started_by(egui::PointerButton::Primary) {
            self.begin(start());
        }
        let ours = self.now.as_ref().is_some_and(&mine);
        match drag {
            Drag::Moving(_) if !ours => Drag::None,
            Drag::Done(_) if response.drag_stopped() && self.end_if(&mine).is_none() => Drag::None,
            other => other,
        }
    }

    /// The automation point held: dragged by itself, or made by a press on the line and held since.
    pub(crate) fn lane_point(&self) -> Option<&PointDrag> {
        match &self.now {
            Some(Gesture::LanePoint(drag) | Gesture::NewPoint(_, Some(drag))) => Some(drag),
            _ => None,
        }
    }

    pub(crate) fn lane_point_mut(&mut self) -> Option<&mut PointDrag> {
        match &mut self.now {
            Some(Gesture::LanePoint(drag) | Gesture::NewPoint(_, Some(drag))) => Some(drag),
            _ => None,
        }
    }

    /// Lets go of the automation point held when `mine` says it is the caller's, handing it back to
    /// commit. A point made by a press stays made: the press goes on until the button comes up.
    pub(crate) fn end_lane_point(&mut self, mine: impl FnOnce(&PointDrag) -> bool) -> Option<PointDrag> {
        match &mut self.now {
            Some(Gesture::NewPoint(_, held)) => held.take_if(|d| mine(d)),
            Some(Gesture::LanePoint(drag)) if mine(drag) => match self.now.take() {
                Some(Gesture::LanePoint(drag)) => Some(drag),
                _ => None,
            },
            _ => None,
        }
    }

    /// The point a press on an automation line made, while that press goes on.
    pub(crate) fn new_point(&self) -> Option<&NewPoint> {
        match &self.now {
            Some(Gesture::NewPoint(made, _)) => Some(made),
            _ => None,
        }
    }

    /// The point a press made is on the lane now: `drag` holds it until the button comes up.
    pub(crate) fn hold_new_point(&mut self, drag: PointDrag) {
        if let Some(Gesture::NewPoint(_, held @ None)) = &mut self.now {
            *held = Some(drag);
        }
    }

    /// Ends a press on an automation line when `mine` says it is the caller's, with the point it
    /// held.
    pub(crate) fn end_new_point(&mut self, mine: impl FnOnce(&NewPoint) -> bool) -> Option<NewPoint> {
        match self.end_if(|g| matches!(g, Gesture::NewPoint(made, _) if mine(made))) {
            Some(Gesture::NewPoint(made, _)) => Some(made),
            _ => None,
        }
    }

    /// Ends whatever drag is under way without committing it, as Escape or a switch to another
    /// project does, and lets the rest of its press do nothing until every button is up. Returns
    /// whether one was.
    pub(crate) fn cancel(&mut self) -> bool {
        self.cancelled = true;
        self.now.take().is_some()
    }

    /// Whether the press held now is one Escape let go of: it starts and finishes nothing.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Called once a frame after every widget had its release: with every button up, a drag still
    /// under way lost its release (its widget was not drawn when the button went up) and ends
    /// without doing anything, so it cannot hold up the next drag; and a press Escape let go of is
    /// over.
    pub(crate) fn settle(&mut self, buttons_down: bool) {
        if !buttons_down {
            self.now = None;
            self.cancelled = false;
        }
    }
}

/// For each variant: the drag it carries while that kind is under way, the same to change it as
/// the pointer moves, and an end that hands it back when `mine` says it is the caller's.
macro_rules! kinds {
    ($($variant:ident($ty:ty) => $get:ident, $get_mut:ident, $end:ident;)*) => {
        // Each kind gets all three, whether or not its drag needs them yet.
        #[allow(dead_code)]
        impl GestureSlot {$(
            pub(crate) fn $get(&self) -> Option<&$ty> {
                match &self.now {
                    Some(Gesture::$variant(drag)) => Some(drag),
                    _ => None,
                }
            }

            pub(crate) fn $get_mut(&mut self) -> Option<&mut $ty> {
                match &mut self.now {
                    Some(Gesture::$variant(drag)) => Some(drag),
                    _ => None,
                }
            }

            pub(crate) fn $end(&mut self, mine: impl FnOnce(&$ty) -> bool) -> Option<$ty> {
                match self.end_if(|g| matches!(g, Gesture::$variant(drag) if mine(drag))) {
                    Some(Gesture::$variant(drag)) => Some(drag),
                    _ => None,
                }
            }
        )*}
    };
}

kinds! {
    Marquee(Marquee) => marquee, marquee_mut, end_marquee;
    Draw((TrackId, f64)) => draw, draw_mut, end_draw;
    Clip(ClipDrag) => clip, clip_mut, end_clip;
    Edge(EdgeDrag) => edge, edge_mut, end_edge;
    Fade(FadeDrag) => fade, fade_mut, end_fade;
    FadeCurve(CurveDrag) => fade_curve, fade_curve_mut, end_fade_curve;
    Gain(((TrackId, ClipId), f64)) => gain, gain_mut, end_gain;
    Value(ValueDrag) => value, value_mut, end_value;
    Main(f64) => main, main_mut, end_main;
    Send((TrackId, TrackId, f64)) => send, send_mut, end_send;
    Track(TrackId) => track, track_mut, end_track;
    Marker((usize, f64)) => marker, marker_mut, end_marker;
    Loop((LoopGrab, f64)) => loop_brace, loop_brace_mut, end_loop_brace;
    Take((TrackId, usize, f64)) => take, take_mut, end_take;
    TakeArea((TrackId, usize, f64)) => take_area, take_area_mut, end_take_area;
    TakeMove(TakeMove) => take_move, take_move_mut, end_take_move;
    TakeEdge(TakeEdge) => take_edge, take_edge_mut, end_take_edge;
    CompFade((ClipId, f64)) => comp_fade, comp_fade_mut, end_comp_fade;
    CompCurve((ClipId, f64, f32)) => comp_curve, comp_curve_mut, end_comp_curve;
    LaneCurve(lanes::CurveDrag) => lane_curve, lane_curve_mut, end_lane_curve;
    LaneBox(BoxDrag) => lane_box, lane_box_mut, end_lane_box;
    LaneHeaders(HeaderDrag) => lane_headers, lane_headers_mut, end_lane_headers;
}
