// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A header's box tree under the user-agent sheet alone lays its parts out where Suhina's own
//! layout put them before it moved into the sheet (#1453): [`native`] is that layout, kept as the
//! oracle.

use super::*;

/// Suhina's own layout of a header in `rect` (its content box), from before #1453.
fn native(rect: Rect, header: Header) -> Parts {
    let inside = rect;
    let spacious = inside.height() >= 60.0;
    let tiny = inside.height() < 20.0;
    let header_row = if tiny {
        (inside.height() - 2.0).max(1.0)
    } else if spacious {
        20.0
    } else {
        HEADER_ROW
    };
    let field_height = if spacious { 20.0 } else { FIELD };
    let header_pad = if tiny {
        1.0
    } else if inside.height() < 48.0 {
        2.0
    } else if spacious {
        6.0
    } else {
        HEADER_PAD_Y
    };
    let header_gap = if inside.height() < 48.0 {
        4.0
    } else if spacious {
        10.0
    } else {
        PAD
    };
    let mut parts = Parts::default();
    let mut band_left = rect.left();
    let indent = (0..header.ancestors).map(|level| nesting_band_width(level) + 2.0).sum::<f32>();
    let indent_room = (rect.width() - COLOUR_BAND - 3.0 * PAD - meter::WIDTH - 114.0).max(0.0);
    let indent_scale = if indent > 0.0 { (indent_room / indent).min(1.0) } else { 1.0 };
    for level in 0..header.ancestors {
        band_left += nesting_band_width(level) * indent_scale + 2.0 * indent_scale;
    }
    if header.ancestors > 0 {
        parts.indent = Some(Rect::from_min_max(rect.min, Pos2::new(band_left, rect.bottom())));
    }
    parts.indent_scale = indent_scale;
    let band = Rect::from_min_size(
        Pos2::new(band_left, rect.top()),
        Vec2::new(nesting_band_width(header.depth), rect.height()),
    );
    parts.band = Some(band);
    let meter_x = rect.right() - PAD - meter::WIDTH;
    let inner = Rect::from_min_max(
        Pos2::new(band.right() + PAD, rect.top() + header_pad),
        Pos2::new(meter_x - PAD, rect.bottom() - header_pad),
    );
    let narrow = rect.height() >= SHOWS_MIXER
        && inner.width() < FOLD + 2.0 + SOLO + PAD + ON_TOGGLE + 3.0 * (SOLO + 2.0) + PAD + LEAST_NAME;
    let solo_width = if narrow || rect.height() < SHOWS_MIXER { 18.0 } else { SOLO };
    let on_width = if narrow || rect.height() < SHOWS_MIXER { 24.0 } else { ON_TOGGLE };
    let meter_bottom = if rect.height() >= SHOWS_MIXER {
        inner.top() + header_row + header_gap + field_height
    } else {
        inner.top() + header_row
    };
    parts.meter = Some(Rect::from_min_max(
        Pos2::new(meter_x, inner.top()),
        Pos2::new(meter_x + meter::WIDTH, meter_bottom.min(inner.bottom())),
    ));
    parts.fold = Some(Rect::from_min_size(inner.min, Vec2::new(FOLD, header_row)));
    let first_x = inner.left() + FOLD + 2.0;
    let name_first = rect.height() >= SHOWS_MIXER
        && inner.width() - FOLD - 2.0 - on_width - 3.0 * (solo_width + 2.0) - PAD - PAN_FIELD - 4.0 >= LEAST_VOLUME;
    let automation_x = first_x;
    let first_x = if narrow {
        inner.left()
    } else if name_first {
        first_x
    } else {
        first_x + solo_width + PAD
    };
    let row_top = if name_first || narrow { inner.top() + header_row + header_gap } else { inner.top() };
    let row = Rect::from_min_max(Pos2::new(first_x, row_top), Pos2::new(inner.right(), row_top + header_row));
    let toggle = Rect::from_min_size(row.min, Vec2::new(on_width, header_row));
    let solo = Rect::from_min_size(Pos2::new(toggle.right() + 2.0, row.top()), Vec2::new(solo_width, header_row));
    let cue = Rect::from_min_size(Pos2::new(solo.right() + 2.0, row.top()), Vec2::new(solo_width, header_row));
    let arm = header
        .armable
        .then(|| Rect::from_min_size(Pos2::new(cue.right() + 2.0, row.top()), Vec2::new(solo_width, header_row)));
    let monitor_width = if rect.height() < SHOWS_MIXER { 24.0 } else { 32.0 };
    let has_takes = header.takes;
    let monitor_end = if has_takes && !narrow { row.right() - solo_width - 4.0 } else { row.right() };
    let monitor = header
        .notes
        .then(|| {
            Rect::from_min_size(
                Pos2::new(arm.unwrap_or(cue).right() + 2.0, row.top()),
                Vec2::new(monitor_width, header_row),
            )
        })
        .filter(|at| at.right() <= monitor_end);
    let controls_end = monitor.unwrap_or(arm.unwrap_or(cue)).right();
    let tall = rect.height() >= SHOWS_MIXER;
    let beside_room = row.right() - PAN_FIELD - 4.0 - (controls_end + PAD);
    let side = tall && beside_room >= LEAST_VOLUME;
    let stacked = tall && !side;
    let below_top = if name_first || narrow { inner.top() } else { row.bottom() + header_gap };
    let below_left = if stacked { automation_x } else { first_x };
    let below =
        Rect::from_min_max(Pos2::new(below_left, below_top), Pos2::new(inner.right(), below_top + field_height));
    let lanes_button = if narrow {
        Rect::from_min_size(Pos2::new(inner.right() - solo_width, inner.top()), Vec2::new(solo_width, header_row))
    } else if side {
        Rect::from_min_size(
            Pos2::new(below.left() + (on_width - solo_width) / 2.0, below.top()),
            Vec2::new(solo_width, header_row),
        )
    } else {
        Rect::from_min_size(Pos2::new(automation_x, row.top()), Vec2::new(solo_width, header_row))
    };
    let takes_button = has_takes.then(|| {
        if side {
            lanes_button.translate(Vec2::new(SOLO + 2.0, 0.0))
        } else if narrow {
            lanes_button.translate(Vec2::new(-(SOLO + 2.0), 0.0))
        } else {
            Rect::from_min_size(Pos2::new(row.right() - solo_width, row.top()), Vec2::new(solo_width, header_row))
        }
    });
    let lanes_right = takes_button.filter(|_| side).unwrap_or(lanes_button).right();
    let step = SEND_WIDTH + SEND_GAP;
    let stacked_pan =
        Rect::from_min_max(Pos2::new(below.right() - PAN_FIELD, below.top()), Pos2::new(below.right(), below.bottom()));
    let send_room = if side {
        below.right() - lanes_right - PAD - LEAST_NAME
    } else if stacked {
        stacked_pan.left() - 4.0 - below.left() - LEAST_VOLUME
    } else {
        0.0
    };
    let overflow = header.sends > (send_room / step).floor().max(0.0) as usize;
    let navigation = if overflow && send_room >= step + 36.0 { 36.0 } else { 0.0 };
    let shown_sends = header.sends.min(((send_room - navigation) / step).floor().max(0.0) as usize);
    let sends_left = if stacked {
        stacked_pan.left() - 4.0 - step * shown_sends as f32 - navigation + SEND_GAP
    } else {
        below.right() - step * shown_sends as f32 - navigation + SEND_GAP
    };
    let mut name = if narrow {
        Rect::from_min_max(
            Pos2::new(automation_x, inner.top()),
            Pos2::new(lanes_button.left() - PAD, inner.top() + header_row),
        )
    } else if side {
        Rect::from_min_max(
            Pos2::new(lanes_right + PAD, below.top()),
            Pos2::new(sends_left - SEND_GAP - 4.0, below.top() + header_row),
        )
    } else {
        let end = takes_button.map_or(row.right(), |b| b.left() - 4.0);
        Rect::from_min_max(Pos2::new(controls_end + PAD, row.top()), Pos2::new(end, row.bottom()))
    };
    if !tall && name.width() >= 28.0 + PAD + 48.0 {
        let width = (name.width() * 0.45).clamp(48.0, 80.0);
        let volume = Rect::from_min_max(Pos2::new(name.right() - width, row.top()), name.right_bottom());
        name.max.x = volume.left() - PAD;
        parts.volume = Some(volume);
    }
    parts.name = Some(name);
    parts.automation = Some(lanes_button);
    parts.takes = takes_button;
    parts.track_on = Some(toggle);
    parts.solo = Some(solo);
    parts.cue = Some(cue);
    parts.arm = arm;
    parts.monitor = monitor;
    if narrow {
        parts.volume = Some(Rect::from_min_max(Pos2::new(controls_end + PAD, row.top()), row.right_bottom()));
    }
    parts.shown_sends = if tall && !narrow { shown_sends } else { 0 };
    parts.navigation = tall && !narrow && navigation > 0.0 && shown_sends > 0;
    if tall && !narrow {
        let (volume, pan) = if side {
            let left = controls_end + PAD;
            let pan =
                Rect::from_min_max(Pos2::new(row.right() - PAN_FIELD, row.top()), Pos2::new(row.right(), row.bottom()));
            (Rect::from_min_max(Pos2::new(left, row.top()), Pos2::new((pan.left() - 4.0).max(left), row.bottom())), pan)
        } else {
            let right = (sends_left - SEND_GAP).max(below.left());
            (Rect::from_min_max(below.min, Pos2::new(right, below.bottom())), stacked_pan)
        };
        parts.volume = Some(volume);
        parts.pan = Some(pan);
        if parts.navigation {
            let at = |index: usize| {
                Rect::from_min_size(
                    Pos2::new(sends_left + step * shown_sends as f32 + index as f32 * 18.0, below.top()),
                    Vec2::new(16.0, field_height),
                )
            };
            parts.previous = Some(at(0));
            parts.next = Some(at(1));
        }
        parts.sends = (0..shown_sends)
            .map(|index| {
                Some(Rect::from_min_size(
                    Pos2::new(sends_left + step * index as f32, below.top()),
                    Vec2::new(SEND_WIDTH, field_height),
                ))
            })
            .collect();
    }
    if rect.height() >= SHOWS_ROUTING {
        let top = row.bottom().max(below.bottom()) + PAD + 4.0;
        let line = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + DROPDOWN));
        if line.bottom() <= inner.bottom() {
            parts.output = Some(line);
        }
        let input = line.translate(Vec2::new(0.0, DROPDOWN + PAD));
        if rect.height() >= SHOWS_MIDI_INPUT && input.bottom() <= inner.bottom() {
            parts.input = Some(input);
        }
    }
    parts.row = header_row;
    parts.name_size = if tiny {
        10.0
    } else if spacious {
        theme::TEXT + 2.0
    } else {
        theme::TEXT
    };
    parts
}

fn close(a: Option<Rect>, b: Option<Rect>) -> bool {
    // A part with no room had a rectangle turned inside out; it now has none of its width.
    let b = b.map(|b| if b.width() < 0.0 { Rect::from_min_max(b.min, Pos2::new(b.left(), b.bottom())) } else { b });
    match (a, b) {
        (Some(a), Some(b)) => (a.min - b.min).length() < 1e-3 && (a.max - b.max).length() < 1e-3,
        (None, None) => true,
        _ => false,
    }
}

/// The parts that differ between two layouts, by name.
fn differ(a: &Parts, b: &Parts) -> Vec<&'static str> {
    let mut differ = Vec::new();
    for (name, x, y) in [
        ("indent", a.indent, b.indent),
        ("band", a.band, b.band),
        ("meter", a.meter, b.meter),
        ("fold", a.fold, b.fold),
        ("automation", a.automation, b.automation),
        ("takes", a.takes, b.takes),
        ("track-on", a.track_on, b.track_on),
        ("solo", a.solo, b.solo),
        ("cue", a.cue, b.cue),
        ("arm", a.arm, b.arm),
        ("monitor", a.monitor, b.monitor),
        ("name", a.name, b.name),
        ("volume", a.volume, b.volume),
        ("pan", a.pan, b.pan),
        ("previous", a.previous, b.previous),
        ("next", a.next, b.next),
        ("output", a.output, b.output),
        ("input", a.input, b.input),
    ] {
        if !close(x, y) {
            differ.push(name);
        }
    }
    if a.sends.len() != b.sends.len() || a.sends.iter().zip(&b.sends).any(|(x, y)| !close(*x, *y)) {
        differ.push("sends");
    }
    if (a.shown_sends, a.navigation, a.row, a.name_size) != (b.shown_sends, b.navigation, b.row, b.name_size) {
        differ.push("counts");
    }
    differ
}

/// Whether a narrow header's monitor button left its volume no room (a volume turned inside out).
fn narrow_volume_squeezed(want: &Parts) -> bool {
    want.monitor.is_some() && want.volume.is_some_and(|volume| volume.width() <= 0.0) && want.pan.is_none()
}

#[test]
fn with_no_rule_the_header_lays_out_as_suhina_s_own_layout_did() {
    let mut checked = 0;
    for width in [160.0, 180.0, 200.0, 230.0, 260.0, 300.0, 340.0, 420.0, 520.0] {
        for height in [14.0, 18.0, 22.0, 30.0, 39.0, 40.0, 47.0, 48.0, 59.0, 60.0, 70.0, 75.0, 90.0, 95.0, 120.0] {
            for (depth, ancestors) in [(0, 0), (1, 1), (3, 3)] {
                for (armable, notes, takes, sends) in
                    [(true, true, false, 2), (false, false, true, 0), (true, false, true, 5), (true, true, true, 1)]
                {
                    let header = Header { depth, ancestors, armable, notes, takes, sends, ..Header::default() };
                    let rect = Rect::from_min_size(Pos2::new(11.0, 5.0), Vec2::new(width, height));
                    let got = layout(None, rect, header);
                    let want = native(rect, header);
                    let mut wrong = differ(&got, &want);
                    // With no room for the name the take lanes' button stood at the line's end,
                    // over the buttons; it now follows them.
                    if want.name.is_some_and(|name| name.width() < 0.0) {
                        wrong.retain(|part| *part != "takes");
                    }
                    // A narrow header with no room left for its volume after the monitor button
                    // turned the volume inside out; the monitor button now gives way to it.
                    if narrow_volume_squeezed(&want) {
                        wrong.retain(|part| !matches!(*part, "monitor" | "volume"));
                    }
                    // A narrow header with room beside its buttons put that button past its
                    // lanes' button, over the meter; it now stands before it.
                    let narrow = want.track_on.zip(want.fold).is_some_and(|(on, fold)| on.left() == fold.left());
                    if narrow && want.takes.zip(want.automation).is_some_and(|(t, a)| t.left() > a.right()) {
                        wrong.retain(|part| *part != "takes");
                    }
                    // A stacked header whose name comes first put its lanes' button under its
                    // track-on switch; it now stands before it.
                    let overlapped = want.automation.zip(want.track_on).is_some_and(|(a, b)| a.intersects(b));
                    if !overlapped {
                        assert!(wrong.is_empty(), "{width}×{height} {header:?}: {wrong:?}\n{got:#?}\n{want:#?}");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 1000, "{checked}");
}

#[test]
fn a_header_s_words_change_only_where_it_crosses_its_sizes() {
    // #1489: a drag of the header column changes the header's width every frame; its attributes,
    // which are in the style tree, change only at a few widths, never back and forth.
    for height in [22.0, 40.0, 60.0, 90.0] {
        let header = Header { armable: true, notes: true, sends: 2, ..Header::default() };
        let mut seen: Vec<(&str, &str, bool)> = Vec::new();
        let mut changes = 0;
        let mut width = 120.0;
        while width <= 900.0 {
            let rect = Rect::from_min_size(Pos2::new(11.0, 5.0), Vec2::new(width, height));
            let words = layout(None, rect, header).words;
            if seen.last() != Some(&words) {
                assert!(!seen.contains(&words), "{height}: {words:?} came back at {width}, after {seen:?}");
                seen.push(words);
                changes += 1;
            }
            width += 1.0;
        }
        assert!(changes <= 4, "{height}: the words changed {changes} times: {seen:?}");
        // The size word follows the height alone.
        assert!(seen.iter().all(|(_, size, _)| *size == seen[0].1), "{height}: {seen:?}");
    }
}

#[test]
fn compact_headers_keep_the_fold_target_wide_and_clear_of_other_controls() {
    for height in [20.0, 22.0, 26.0, 40.0] {
        for width in [220.0, 300.0, 460.0] {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
            let parts = layout(None, rect, Header::default());
            let fold = parts.fold.unwrap();
            let target = parts.fold_target(rect).unwrap();
            assert!(fold.width() >= 20.0, "{width}×{height}: {fold:?}");
            assert!(target.contains_rect(fold), "the target keeps the glyph's reserved area");
            for control in [parts.automation, parts.track_on, parts.solo, parts.cue, parts.name, parts.volume]
                .into_iter()
                .flatten()
            {
                assert!(!fold.intersects(control), "{width}×{height}: {fold:?} overlaps {control:?}");
                assert!(!target.intersects(control), "{width}×{height}: {target:?} overlaps {control:?}");
            }
        }
    }
}

#[test]
fn disclosure_uses_the_free_gutter_and_stops_at_other_controls() {
    let bounds = Rect::from_min_size(Pos2::ZERO, Vec2::new(240.0, 60.0));
    let fold = Rect::from_min_size(Pos2::new(10.0, 4.0), Vec2::new(20.0, 14.0));
    let mut parts = Parts { fold: Some(fold), ..Parts::default() };
    assert_eq!(parts.fold_target(bounds).unwrap().height(), 60.0);
    parts.automation = Some(Rect::from_min_size(Pos2::new(10.0, 30.0), Vec2::splat(14.0)));
    assert_eq!(parts.fold_target(bounds).unwrap().bottom(), 29.0);
    assert_eq!(parts.fold_target(bounds).unwrap().width(), fold.width());
}

#[test]
fn containers_reserve_the_whole_inner_edge_on_either_header_side() {
    for headers_left in [false, true] {
        for height in [22.0, 60.0, 95.0, 160.0] {
            for width in [220.0, 300.0, 460.0] {
                for depth in [0, 2] {
                    let bounds = Rect::from_min_size(Pos2::new(11.0, 5.0), Vec2::new(width, height));
                    let parts = layout(
                        None,
                        bounds,
                        Header {
                            container: true,
                            headers_left,
                            depth,
                            ancestors: depth,
                            armable: true,
                            notes: true,
                            takes: true,
                            sends: 2,
                        },
                    );
                    let target = parts.fold_target(bounds).unwrap();
                    let fold = parts.fold.unwrap();
                    assert_eq!(target.top(), bounds.top());
                    assert_eq!(target.bottom(), bounds.bottom());
                    assert!(target.contains_rect(fold), "{parts:?}");
                    assert_eq!(target.width(), FOLD);
                    assert_eq!(target.center().x > bounds.center().x, headers_left);
                    for control in [
                        parts.band,
                        parts.indent,
                        parts.meter,
                        parts.name,
                        parts.automation,
                        parts.takes,
                        parts.track_on,
                        parts.solo,
                        parts.cue,
                        parts.arm,
                        parts.monitor,
                        parts.volume,
                        parts.pan,
                        parts.previous,
                        parts.next,
                        parts.output,
                        parts.input,
                    ]
                    .into_iter()
                    .chain(parts.sends.iter().copied())
                    .flatten()
                    {
                        assert!(
                            !target.intersects(control),
                            "{width}×{height}, left={headers_left}: {target:?} overlaps {control:?}"
                        );
                    }
                }
            }
        }
    }
}
