// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The strip's box tree under the user-agent sheet alone lays its parts out where Suhina's own
//! layout put them before it moved into the sheet (#1453): [`native`] is that layout, kept as the
//! oracle.

use super::*;

/// Where Suhina's own layout put a strip's parts, before the sheet laid them out.
#[derive(Debug, Default, PartialEq)]
struct Native {
    band: Option<Rect>,
    name: Option<Rect>,
    output: Option<Rect>,
    input: Option<[Rect; 3]>,
    devices: Option<Rect>,
    divider: Option<Rect>,
    sends: Option<Rect>,
    pan: Option<Rect>,
    readout: Option<Rect>,
    fader: Option<Rect>,
    scale: Option<Rect>,
    meter: Option<Rect>,
    compact: Option<Rect>,
    toggles: Option<Rect>,
    buttons: Vec<Rect>,
}

/// Suhina's own layout of a strip in `rect`, from before #1453; the readout is the 20 points over
/// the fader it was drawn in.
fn native(rect: Rect, has_sends: bool, input: bool, shared: Shared, main: bool, armable: bool) -> Native {
    let inner = rect.shrink(PAD);
    let height = rect.height();
    let footer = if height >= SENDS_AT { SEND_HEIGHT + SEND_GAP } else { 0.0 };
    let toggle_bottom = inner.bottom() - footer;
    let toggles = Rect::from_min_max(
        Pos2::new(inner.left(), (toggle_bottom - theme::TOGGLE).max(inner.top())),
        Pos2::new(inner.right(), toggle_bottom),
    );
    let mut parts = Native {
        band: Some(Rect::from_min_size(rect.min, Vec2::new(COLOUR_BAND, rect.height()))),
        toggles: Some(toggles),
        ..Default::default()
    };
    let row = |top: f32, height: f32| {
        Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + height))
    };
    if main {
        parts
            .buttons
            .push(Rect::from_min_size(toggles.min, Vec2::new(28.0_f32.min(toggles.width()), toggles.height())));
    } else {
        let small = if armable { (toggles.width() - 6.0) / 5.0 } else { (toggles.width() - 4.0) / 4.0 };
        let mut right = toggles.right();
        let mut buttons = Vec::new();
        for _ in 0..if armable { 3 } else { 2 } {
            buttons
                .push(Rect::from_min_max(Pos2::new(right - small, toggles.top()), Pos2::new(right, toggles.bottom())));
            right -= small + 2.0;
        }
        buttons.push(Rect::from_min_max(toggles.min, Pos2::new(right, toggles.bottom())));
        buttons.reverse();
        parts.buttons = buttons;
    }
    let mut bottom = toggles.top() - ABOVE_TOGGLES;
    if (input || shared.reserve_input) && height >= INPUT_AT {
        bottom -= AUDIO_MONITOR;
        let monitor = row(bottom, AUDIO_MONITOR);
        bottom -= UNDER_AUDIO + AUDIO_LIST;
        let source = row(bottom, AUDIO_LIST);
        bottom -= UNDER_AUDIO + AUDIO_CAPTION;
        let caption = row(bottom, AUDIO_CAPTION);
        parts.input = input.then_some([caption, source, monitor]);
    }
    if height < FADER_AT {
        if main {
            parts.meter = Some(Rect::from_min_size(
                rect.min + Vec2::splat(PAD),
                Vec2::new(METER_WIDTH, toggles.top() - rect.top() - PAD * 2.0),
            ));
        } else {
            let top = (bottom - COMPACT).max(inner.top());
            parts.compact = Some(row(top, bottom - top)).filter(|r| r.height() > 0.0);
        }
        return parts;
    }
    let mut top = inner.top();
    if height >= NAME_AT {
        parts.name = Some(row(top, NAME));
        top += NAME + UNDER_NAME;
    }
    if height >= OUTPUT_AT {
        parts.output = Some(row(top, OUTPUT));
        top += OUTPUT + UNDER_OUTPUT;
    }
    let pan = height >= PAN_AT;
    if height >= SENDS_AT && has_sends {
        parts.sends = Some(row(toggles.bottom() + SEND_GAP, SEND_HEIGHT));
    }
    let fixed = READOUT + UNDER_READOUT + if pan { KNOB_ROW } else { 0.0 };
    let rest = (bottom - top - fixed).max(0.0);
    let fader = if height >= DEVICES_AT {
        let room = rest;
        let fader = if shared.fader > 0.0 {
            shared.fader.clamp(LEAST_FADER, (room - DEVICE_ROW - UNDER_DEVICES).max(LEAST_FADER))
        } else {
            (room - LEAST_DEVICES * DEVICE_ROW - UNDER_DEVICES).clamp(LEAST_FADER, TALLEST_FADER_WITH_DEVICES)
        };
        let list = room - fader - UNDER_DEVICES;
        if list >= DEVICE_ROW {
            parts.devices = Some(row(top, list));
            top += list + UNDER_DEVICES;
            fader
        } else {
            rest
        }
    } else {
        rest
    };
    if pan {
        parts.pan = Some(row(top, theme::KNOB));
        top += KNOB_ROW;
    }
    let centred = rect.width() <= theme::SCENE_COLUMN;
    let group_left = if centred { inner.center().x - FADER_COLUMN / 2.0 } else { inner.left() };
    let column =
        |top: f32, height: f32| Rect::from_min_size(Pos2::new(group_left, top), Vec2::new(FADER_COLUMN, height));
    parts.readout = Some(column(top, READOUT));
    top += READOUT + UNDER_READOUT;
    let fader = column(top, fader.min(bottom - top).max(0.0));
    parts.meter = Some(Rect::from_min_size(
        Pos2::new(fader.right() + if centred { 0.0 } else { SCALE_WIDTH } + 4.0, fader.top()),
        Vec2::new(METER_WIDTH, fader.height()),
    ));
    parts.scale = Some(Rect::from_min_size(
        Pos2::new(if centred { fader.left() - SCALE_WIDTH - 2.0 } else { fader.right() }, fader.top()),
        Vec2::new(SCALE_WIDTH, fader.height()),
    ));
    parts.fader = Some(fader);
    parts.divider = parts.devices.map(|list| {
        let line = list.bottom() + UNDER_DEVICES / 2.0;
        Rect::from_x_y_ranges(inner.x_range(), egui::Rangef::new(line - 3.0, line + 3.0))
    });
    parts
}

/// What the sheet's layout gives, in the oracle's terms.
fn laid(layout: &Layout, main: bool) -> Native {
    let buttons: Vec<Rect> = if main {
        layout.metronome.into_iter().collect()
    } else {
        [layout.track_on, layout.solo, layout.cue, layout.arm].into_iter().flatten().collect()
    };
    Native {
        band: layout.band,
        name: layout.name,
        output: layout.output,
        input: layout.audio_caption.zip(layout.audio_source).zip(layout.audio_monitor).map(|((a, b), c)| [a, b, c]),
        devices: layout.devices,
        divider: layout.divider,
        sends: layout.sends,
        pan: layout.pan,
        readout: layout.readout,
        fader: layout.fader,
        scale: layout.scale,
        meter: layout.meter,
        compact: layout.compact,
        toggles: Some(layout.toggles),
        buttons,
    }
}

/// Whether two layouts agree to a thousandth of a point (Taffy adds up in another order).
fn same(a: &Native, b: &Native) -> bool {
    let close = |a: &Option<Rect>, b: &Option<Rect>| match (a, b) {
        (Some(a), Some(b)) => (a.min - b.min).length() < 1e-3 && (a.max - b.max).length() < 1e-3,
        (None, None) => true,
        _ => false,
    };
    let all = |a: &[Rect], b: &[Rect]| a.len() == b.len() && a.iter().zip(b).all(|(a, b)| close(&Some(*a), &Some(*b)));
    close(&a.band, &b.band)
        && close(&a.name, &b.name)
        && close(&a.output, &b.output)
        && match (&a.input, &b.input) {
            (Some(a), Some(b)) => all(a, b),
            (None, None) => true,
            _ => false,
        }
        && close(&a.devices, &b.devices)
        && close(&a.divider, &b.divider)
        && close(&a.sends, &b.sends)
        && close(&a.pan, &b.pan)
        && close(&a.readout, &b.readout)
        && close(&a.fader, &b.fader)
        && close(&a.scale, &b.scale)
        && close(&a.meter, &b.meter)
        && close(&a.compact, &b.compact)
        && close(&a.toggles, &b.toggles)
        && all(&a.buttons, &b.buttons)
}

#[test]
fn with_no_rule_the_strip_lays_out_as_suhina_s_own_layout_did() {
    let mut checked = 0;
    for width in [64.0, theme::RETURN_COLUMN, 88.0, theme::SCENE_COLUMN, 97.0, theme::TRACK_COLUMN, 160.0] {
        for height in (40..460).step_by(3).map(|h| h as f32) {
            for (sends, input, reserve, armable) in [
                (true, false, false, true),
                (false, true, false, true),
                (false, false, true, false),
                (true, true, true, true),
            ] {
                for fader in [0.0, 90.0] {
                    let shared = Shared { reserve_input: reserve, fader };
                    let rect = Rect::from_min_size(Pos2::new(13.0, 7.0), Vec2::new(width, height));
                    let kind = Strip::Track { armable };
                    let got = laid(&layout_in(Host::Alone, rect, sends, input, shared, kind), false);
                    let want = native(rect, sends, input, shared, false, armable);
                    assert!(
                        same(&got, &want),
                        "{width}×{height} {sends} {input} {reserve} {armable} {fader}:\n{got:#?}\n{want:#?}"
                    );
                    let main = laid(&layout_in(Host::Alone, rect, false, false, shared, Strip::Main), true);
                    let want = native(rect, false, false, shared, true, false);
                    assert!(same(&main, &want), "Main {width}×{height}:\n{main:#?}\n{want:#?}");
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 1000);
}
