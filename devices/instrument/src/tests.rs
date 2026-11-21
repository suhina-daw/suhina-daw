// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

suhina_device_api::conformance_test!(Instrument::with_rate);

#[test]
fn revision_one_matches_the_independent_analytical_reference_render() {
    let mut instrument = Instrument::new();
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut buffer = vec![0.0; 16384];
    instrument.process(&ProcessContext::default(), &mut buffer);
    for line in include_str!("../tests/data/modal-v1-reference.txt").lines().filter(|line| !line.starts_with('#')) {
        let (frame, expected) = line.split_once(' ').unwrap();
        let frame: usize = frame.parse().unwrap();
        let expected: f32 = expected.parse().unwrap();
        assert!(
            (buffer[frame * 2] - expected).abs() < 3e-7,
            "revision 1 changed at frame {frame}: {} vs {expected}",
            buffer[frame * 2]
        );
        assert_eq!(buffer[frame * 2], buffer[frame * 2 + 1]);
    }
    // A revision-4 voice: 128 fitted resonators with their stereo weights beside the 16 of
    // revisions 1 and 2, its held layer (per-sine levels, ramps and couplings, eleven noise
    // filters) and its tail (eleven bands of three sections in two channels).
    assert!(std::mem::size_of::<Voice>() <= 15 * 1024);
    // The voices live on the heap (allocated at construction), so the Instrument itself stays
    // small enough to move through a debug build's constructors on a 1 MB stack.
    assert!(std::mem::size_of::<Instrument>() <= 32 * 1024);
    assert!(std::mem::size_of::<Instrument>() + MAX_VOICES * std::mem::size_of::<Voice>() <= 256 * 1024);
}

#[test]
fn modal_notes_are_audible_and_release_and_reset_are_exactly_silent() {
    let mut instrument = Instrument::new();
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut buffer = [0.0; 1024];
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().any(|value| value.abs() > 0.001));
    instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    for _ in 0..32 {
        instrument.process(&ProcessContext::default(), &mut buffer);
    }
    assert!(buffer.iter().all(|value| *value == 0.0));
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    instrument.reset();
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().all(|value| *value == 0.0));
}

#[test]
fn unsupported_model_revisions_do_not_silently_play_a_different_model() {
    let mut instrument = Instrument::new();
    instrument.set_param(param::REVISION, 4.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut buffer = [1.0; 1024];
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(!instrument.model_supported());
    assert!(buffer.iter().all(|value| *value == 0.0));
    assert_eq!(instrument.param(param::REVISION), Some(4.0));
}

#[test]
fn single_sample_maps_play_through_the_shared_runtime() {
    let mut instrument = Instrument::new();
    instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.5; 4800]));
    instrument.set_param(param::SOURCE, 1.0);
    instrument.set_param(param::ATTACK, 0.0);
    instrument.set_param(param::GAIN, 0.0);
    instrument.set_param(param::KEY_LOW, 60.0);
    instrument.set_param(param::KEY_HIGH, 60.0);
    instrument.reset();
    let mut buffer = [0.0; 1024];
    instrument.event(NoteEvent::On { key: 59, velocity: 1.0 });
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().all(|value| *value == 0.0));
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    instrument.process(&ProcessContext::default(), &mut buffer);
    assert!((buffer[512] - 0.5).abs() < 1e-6);
}

#[test]
fn key_parameters_are_shown_as_note_names() {
    use suhina_device_sdk::ui::{Draw, TextFormat, Widget};
    for index in [param::ROOT, param::KEY_LOW, param::KEY_HIGH] {
        assert_eq!(PARAMS[index].unit, Unit::Key, "{}", PARAMS[index].id);
    }
    let layout = panel::layout();
    for canvas in [layout.ui.as_ref().unwrap(), layout.expanded.as_ref().unwrap()] {
        for id in ["root", "key_low", "key_high"] {
            for control in canvas.controls.iter().filter(|c| bound(c, id)) {
                // A field or dial writes the note's name, and so does a control drawn whole (the
                // root key, large); a marker on the keys writes nothing.
                let drawn_key = control.widget.is_none()
                    && control.drawing.iter().any(|draw| {
                        matches!(draw, Draw::Text { parameter: Some(p), format: TextFormat::Key, .. } if p == id)
                    });
                assert!(
                    drawn_key
                        || matches!(
                            control.widget,
                            Some(
                                Widget::Number { format: TextFormat::Key, .. }
                                    | Widget::Dial { format: TextFormat::Key, .. }
                                    | Widget::Handle { .. }
                            )
                        ),
                    "{id}: {}",
                    control.name
                );
            }
            for control in &canvas.controls {
                if let Some(Widget::Graphic { drawing }) = &control.widget {
                    for draw in drawing {
                        if let Draw::Text { parameter: Some(parameter), format, .. } = draw
                            && parameter == id
                        {
                            assert_eq!(*format, TextFormat::Key, "{id} in {}", control.name);
                        }
                    }
                }
            }
        }
    }
}

/// Whether `control` edits parameter `id`.
fn bound(control: &suhina_device_sdk::ui::CustomControl, id: &str) -> bool {
    [control.x.as_ref(), control.y.as_ref()].into_iter().flatten().any(|b| b.parameter == id)
}

/// Whether `control` shows with these values (every other parameter 0).
fn shown(control: &suhina_device_sdk::ui::CustomControl, values: &[(&str, f64)]) -> bool {
    control.visible_when.as_ref().is_none_or(|condition| {
        condition.evaluate(&|id, _, _| values.iter().find(|(name, _)| *name == id).map_or(0.0, |(_, v)| *v)) > 0.0
    })
}

/// The compact panel opens the editor on the page it showed last: one Open editor button for
/// each page, shown only while that page is the current one, and it expands to that page.
#[test]
fn the_open_editor_button_keeps_the_page_the_editor_showed_last() {
    use suhina_device_sdk::ui::{PAGE_ID, UiGesture};
    let layout = panel::layout();
    let compact = layout.ui.as_ref().unwrap();
    for page in 0..=crate::panel::editor::LAST_PAGE {
        let open: Vec<_> = compact
            .controls
            .iter()
            .filter(|c| c.name == "Open editor" && shown(c, &[("#page", f64::from(page)), ("model_revision", 3.0)]))
            .collect();
        assert_eq!(open.len(), 1, "page {page}");
        assert_eq!(open[0].gesture, UiGesture::Expand(page), "page {page}");
        assert_eq!(open[0].x.as_ref().map(|b| b.parameter.as_str()), Some(PAGE_ID));
    }
    // Every page the editor can show has its own way back into it.
    let editor = layout.expanded.as_ref().unwrap();
    for control in &editor.controls {
        if let UiGesture::Set(page) = control.gesture
            && control.x.as_ref().is_some_and(|b| b.parameter == PAGE_ID)
        {
            assert!(
                page <= crate::panel::editor::LAST_PAGE,
                "{} opens page {page}, which the compact panel cannot reopen",
                control.name
            );
        }
    }
}

/// Every sound parameter has a control in the editor: the strip's on every page, the sample
/// layer's on its page, the keys' and the soft pedal on the keys pages, and Source in the header
/// with any Source. The analysis and the file are on the sample layer, under the waveform.
#[test]
fn every_parameter_is_reachable_and_each_page_carries_its_own() {
    use crate::panel::editor::{ADVANCED, BODY, KEYS_OVER_BODY, KEYS_OVER_SAMPLE, SAMPLE};
    use suhina_device_sdk::ui::{UiGesture, Widget};
    let layout = panel::layout();
    let editor = layout.expanded.as_ref().unwrap();
    for info in PARAMS.iter().filter(|p| p.id != "model_revision") {
        assert!(editor.controls.iter().any(|c| bound(c, &info.id)), "{} has no control", info.id);
    }
    let on = |page: u16, source: f64, id: &str| {
        editor.controls.iter().any(|c| {
            bound(c, id)
                && shown(c, &[("#page", f64::from(page)), ("source", source), ("model_revision", 3.0), ("body", 1.0)])
        })
    };
    for source in [0.0, 1.0, 2.0] {
        for page in [BODY, SAMPLE, ADVANCED, KEYS_OVER_BODY, KEYS_OVER_SAMPLE] {
            for id in ["gain", "release", "blend", "source"] {
                assert!(on(page, source, id), "page {page}: {id}, source {source}");
            }
        }
        for id in [
            "body",
            "mallet_hardness",
            "body_decay",
            "brightness",
            "strike",
            "mallet_weight",
            "resonator",
            "stereo_width",
            "mallet_model",
        ] {
            assert!(on(BODY, source, id), "Body: {id}, source {source}");
        }
        for id in ["root", "playback", "start", "end", "loop_start", "loop_end", "attack", "decay", "sustain"] {
            assert!(on(SAMPLE, source, id), "Sample: {id}, source {source}");
        }
        for page in [KEYS_OVER_BODY, KEYS_OVER_SAMPLE] {
            for id in ["key_low", "key_high", "velocity_low", "velocity_high", "soft_pedal"] {
                assert!(on(page, source, id), "Keys {page}: {id}, source {source}");
            }
        }
        assert!(!on(BODY, source, "soft_pedal") && !on(SAMPLE, source, "key_low"));
    }
    let playback: Vec<_> = editor.controls.iter().filter(|c| bound(c, "playback")).map(|c| c.gesture).collect();
    assert_eq!(playback, [UiGesture::Set(0), UiGesture::Set(1), UiGesture::Set(2)], "Gate, One shot, Loop");
    for widget in [Widget::SampleAnalysis, Widget::SampleFile] {
        let control = editor.controls.iter().find(|c| c.widget.as_ref() == Some(&widget)).unwrap();
        for source in [0.0, 1.0, 2.0] {
            assert!(shown(control, &[("#page", 1.0), ("source", source)]), "{widget:?} source {source}");
        }
        assert!(!shown(control, &[("#page", 0.0)]) && !shown(control, &[("#page", 4.0)]), "{widget:?}");
    }
    let analysis = editor.controls.iter().find(|c| matches!(c.widget, Some(Widget::SampleAnalysis))).unwrap();
    let waves: Vec<_> = editor.controls.iter().filter(|c| matches!(c.widget, Some(Widget::SampleWaveform))).collect();
    assert_eq!(waves.len(), 1, "the waveform belongs to the sample layer alone");
    assert!(waves[0].bounds[1] + waves[0].bounds[3] <= analysis.bounds[1], "the analysis is under the waveform");
    assert!(shown(waves[0], &[("#page", 1.0)]) && !shown(waves[0], &[("#page", 0.0)]));
}

/// The header's layers: a layer's lamp turns it off while both are on and on while it is off
/// (both then play), the one layer on cannot be turned off, and a layer's name shows it. The
/// keys open over the layer shown and Done goes back to it; a page whose layer is off offers to
/// turn it on.
#[test]
fn the_layers_turn_on_and_off_and_the_keys_open_over_the_layer_shown() {
    use crate::panel::editor::{ADVANCED, BODY, KEYS_OVER_BODY, KEYS_OVER_SAMPLE, SAMPLE};
    use suhina_device_sdk::ui::{PAGE_ID, UiGesture};
    let layout = panel::layout();
    for canvas in [layout.expanded.as_ref().unwrap(), layout.ui.as_ref().unwrap()] {
        for (source, body, sample) in [(0, None, Some(2)), (1, Some(2), None), (2, Some(1), Some(0))] {
            let values = [("source", f64::from(source)), ("#page", 0.0), ("model_revision", 3.0)];
            let lamp = |layer: &str| {
                let found: Vec<_> = canvas
                    .controls
                    .iter()
                    .filter(|c| {
                        c.name.starts_with("Turn ") && c.name.ends_with(&format!("{layer} layer")) && shown(c, &values)
                    })
                    .filter(|c| bound(c, "source"))
                    .map(|c| c.gesture)
                    .collect();
                assert!(found.len() <= 1, "{layer}: {found:?}");
                found.first().copied()
            };
            assert_eq!(lamp("body"), body.map(UiGesture::Set), "source {source}");
            assert_eq!(lamp("sample"), sample.map(UiGesture::Set), "source {source}");
        }
    }
    let editor = layout.expanded.as_ref().unwrap();
    let goes = |name: &str, page: u16| {
        let found: Vec<_> = editor
            .controls
            .iter()
            .filter(|c| c.name == name && shown(c, &[("#page", f64::from(page))]))
            .filter(|c| c.x.as_ref().is_some_and(|b| b.parameter == PAGE_ID))
            .map(|c| c.gesture)
            .collect();
        assert_eq!(found.len(), 1, "{name} on page {page}");
        found[0]
    };
    for page in [BODY, SAMPLE, ADVANCED, KEYS_OVER_BODY, KEYS_OVER_SAMPLE] {
        assert_eq!(goes("Show the body layer", page), UiGesture::Set(BODY));
        assert_eq!(goes("Show the sample layer", page), UiGesture::Set(SAMPLE));
    }
    for (from, to) in [
        (BODY, KEYS_OVER_BODY),
        (ADVANCED, KEYS_OVER_BODY),
        (SAMPLE, KEYS_OVER_SAMPLE),
        (KEYS_OVER_BODY, BODY),
        (KEYS_OVER_SAMPLE, SAMPLE),
    ] {
        assert_eq!(goes("Keys and velocity", from), UiGesture::Set(to), "from {from}");
    }
    assert_eq!(goes("Done", KEYS_OVER_BODY), UiGesture::Set(BODY));
    assert_eq!(goes("Done", KEYS_OVER_SAMPLE), UiGesture::Set(SAMPLE));
    for (name, page, off) in [("Turn the body layer on", BODY, 1.0), ("Turn the sample layer on", SAMPLE, 0.0)] {
        for source in [0.0, 1.0, 2.0] {
            let control: Vec<_> = editor
                .controls
                .iter()
                .filter(|c| c.name == name && shown(c, &[("#page", f64::from(page)), ("source", source)]))
                .collect();
            assert_eq!(control.len(), usize::from(source == off), "{name}, source {source}");
            if let Some(control) = control.first() {
                assert_eq!(control.gesture, UiGesture::Set(2));
            }
        }
    }
}

/// The rectangles a dimming veil draws, when `control` is one.
fn veil(control: &suhina_device_sdk::ui::CustomControl) -> Vec<[f32; 4]> {
    use suhina_device_sdk::ui::{Draw, Widget};
    let Some(Widget::Graphic { drawing }) = &control.widget else { return vec![] };
    drawing
        .iter()
        .filter_map(|draw| match draw {
            Draw::Image { asset, rect, tint, .. } if asset == "artwork-light" && tint[3] < 255 => {
                Some(rect.clone().map(|v| v.evaluate(&|_, _, _| 0.0)))
            }
            _ => None,
        })
        .collect()
}

/// Whether some veil of `canvas` shown with `values` covers `control`'s bounds.
fn dimmed(
    canvas: &suhina_device_sdk::ui::DeviceUi,
    control: &suhina_device_sdk::ui::CustomControl,
    values: &[(&str, f64)],
) -> bool {
    let [x, y, w, h] = control.bounds;
    canvas
        .controls
        .iter()
        .filter(|c| shown(c, values))
        .flat_map(veil)
        .any(|[vx, vy, vw, vh]| vx <= x + 0.01 && vy <= y + 0.01 && vx + vw >= x + w - 0.01 && vy + vh >= y + h - 0.01)
}

/// A control with no effect in the current Source is drawn faded (a veil of the artwork over
/// it), and still takes edits: the veils take no input. Release, Gain and the velocities act on
/// both layers and never fade; the envelope stays live for fitted models.
#[test]
fn controls_without_effect_in_the_current_source_are_dimmed() {
    use suhina_device_sdk::ui::{DeviceUi, UiGesture, Widget};
    let layout = panel::layout();
    let editor = layout.expanded.as_ref().unwrap();
    let compact = layout.ui.as_ref().unwrap();
    for canvas in [editor, compact] {
        for control in canvas.controls.iter().filter(|c| !veil(c).is_empty()) {
            assert!(control.x.is_none() && control.y.is_none() && control.gesture == UiGesture::Drag);
            assert!(matches!(control.widget, Some(Widget::Graphic { .. })), "{}", control.name);
        }
    }
    // The control that edits `id` shown with `values`.
    let edits = |canvas: &DeviceUi, id: &str, values: &[(&str, f64)]| {
        canvas.controls.iter().find(|c| bound(c, id) && shown(c, values)).cloned().unwrap()
    };
    for revision in [3.0, 4.0] {
        for source in [0.0, 1.0, 2.0] {
            let at = format!("source {source}, revision {revision}");
            let sample = [("#page", 1.0), ("source", source), ("model_revision", revision), ("body", 1.0)];
            let envelope = source == 0.0 && revision != 4.0;
            for id in ["attack", "decay", "sustain"] {
                assert_eq!(dimmed(editor, &edits(editor, id, &sample), &sample), envelope, "{id} {at}");
            }
            assert_eq!(dimmed(editor, &edits(editor, "root", &sample), &sample), source == 0.0, "{at}");
            for name in ["Source start", "Loop end", "One shot"] {
                let control = editor.controls.iter().find(|c| c.name == name).unwrap();
                assert_eq!(dimmed(editor, control, &sample), source == 0.0, "{name} {at}");
            }
            for page in [0.0, 1.0, 3.0] {
                let values = [("#page", page), ("source", source), ("model_revision", revision), ("body", 1.0)];
                for id in ["release", "gain"] {
                    assert!(!dimmed(editor, &edits(editor, id, &values), &values), "{id} p{page} {at}");
                }
                assert_eq!(
                    dimmed(editor, &edits(editor, "blend", &values), &values),
                    source != 2.0,
                    "blend p{page} {at}"
                );
            }
            let keys = [("#page", 3.0), ("source", source), ("model_revision", revision)];
            for name in ["Lowest velocity", "Highest velocity"] {
                let control = editor.controls.iter().find(|c| c.name == name).unwrap();
                assert!(!dimmed(editor, control, &keys), "{name} {at}");
            }
            let values = [("source", source), ("model_revision", revision), ("body", 1.0)];
            for id in ["body_decay", "brightness"] {
                assert_eq!(dimmed(compact, &edits(compact, id, &values), &values), source == 1.0, "{id} {at}");
            }
            assert_eq!(dimmed(compact, &edits(compact, "blend", &values), &values), source != 2.0, "{at}");
            for id in ["release", "gain"] {
                assert!(!dimmed(compact, &edits(compact, id, &values), &values), "{id} {at}");
            }
        }
    }
}

/// The compact panel: the body's sound with the body layer on, the sample's waveform with the
/// sample layer on, both with both; the body is named for revisions 1 and 3; five dials.
#[test]
fn the_compact_panel_follows_the_layers() {
    use suhina_device_sdk::ui::Widget;
    let layout = panel::layout();
    let compact = layout.ui.as_ref().unwrap();
    for source in [0_u16, 1, 2] {
        let values = [("source", f64::from(source)), ("model_revision", 3.0), ("body", 5.0)];
        let visible = |name: &str| compact.controls.iter().any(|c| c.name == name && shown(c, &values));
        assert_eq!(visible("Sound"), source != 1, "source {source}");
        assert_eq!(
            compact.controls.iter().any(|c| matches!(c.widget, Some(Widget::SampleWaveform)) && shown(c, &values)),
            source != 0,
            "source {source}"
        );
    }
    for (revision, body) in [(1.0, true), (2.0, false), (3.0, true), (4.0, false)] {
        let named = compact.controls.iter().any(|c| c.name == "Body" && shown(c, &[("model_revision", revision)]));
        assert_eq!(named, body, "revision {revision}");
    }
    for name in ["Decay", "Brightness", "Release", "Blend", "Gain", "Open editor"] {
        assert!(compact.controls.iter().any(|c| c.name == name), "{name}");
    }
    let dials = compact.controls.iter().filter(|c| matches!(c.widget, Some(Widget::Dial { .. }))).count();
    assert_eq!(dials, 5);
}

// Both canvases draw the background artwork first, behind every control, with every theme: no
/// sheet hides it, and the sheet colors the controls for it.
#[test]
fn both_canvases_carry_the_light_artwork_behind_their_controls_with_every_theme() {
    let layout = crate::panel::layout();
    for canvas in [layout.ui.as_ref().unwrap(), layout.expanded.as_ref().unwrap()] {
        assert!(canvas.assets.iter().any(|a| a.id == "artwork-light"), "the artwork is embedded");
        assert_eq!(canvas.controls.first().map(|c| c.id.as_str()), Some("artwork-light"));
        let style = canvas.style.as_deref().unwrap_or_default();
        assert!(!style.contains("prefers-color-scheme") && !style.contains("display: none"), "{style}");
        assert!(style.contains("device-control { color: #34261d"), "{style}");
    }
    assert_eq!(layout.expanded.as_ref().unwrap().size, [880.0, 500.0]);
}

/// The Soft pedal switch is on the keys pages, with the keyboard's facts, and Model tune on the
/// body layer's Fitted model group.
#[test]
fn soft_pedal_is_with_the_keys_and_model_tune_on_the_body_layer() {
    let layout = crate::panel::layout();
    let editor = layout.expanded.as_ref().unwrap();
    let switch = editor.controls.iter().find(|c| c.name == "Soft pedal").unwrap();
    assert!(shown(switch, &[("#page", 3.0)]) && shown(switch, &[("#page", 4.0)]) && !shown(switch, &[("#page", 0.0)]));
    let tune = editor.controls.iter().find(|c| c.name == "Model tune").unwrap();
    assert!(shown(tune, &[("#page", 0.0), ("model_revision", 4.0)]));
}

#[test]
fn stock_panel_is_valid_sdk_layout() {
    let panel = panel::layout().panel(&INFO).unwrap();
    assert!(panel.ui.is_some());
}

#[test]
fn sustain_holds_both_hybrid_layers_until_the_same_pedal_release() {
    for source in [0.0, 1.0, 2.0] {
        let mut instrument = Instrument::new();
        instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
        instrument.set_param(param::SOURCE, source);
        instrument.set_param(param::PLAYBACK, 2.0);
        instrument.set_param(param::RELEASE, 0.005);
        instrument.reset();
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
        instrument.event(NoteEvent::On { key: 60, velocity: 0.0 });
        let mut block = [0.0; 1024];
        for _ in 0..4 {
            instrument.process(&ProcessContext::default(), &mut block);
        }
        assert!(block.iter().any(|value| value.abs() > 0.0001), "source {source} must sustain");
        instrument.event(NoteEvent::Controller { number: 64, value: 0.0 });
        for _ in 0..4 {
            instrument.process(&ProcessContext::default(), &mut block);
        }
        assert!(block.iter().all(|value| *value == 0.0), "source {source} must release");
    }
}

#[test]
fn sample_only_notes_never_start_inaudible_modal_voices() {
    let mut instrument = Instrument::new();
    instrument.set_param(param::SOURCE, 1.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    assert!(instrument.voices.iter().all(|voice| !voice.body.is_active()));
}

#[test]
fn source_switches_retire_hidden_sample_voices_independently_of_block_size() {
    fn performance(block_frames: usize) -> Vec<f32> {
        let mut instrument = Instrument::new();
        let source: std::sync::Arc<[f32]> = (0..4800).map(|i| (i as f32 * 0.04).sin() * 0.2).collect();
        instrument.samples().unwrap().load_sample(48000, 1, source);
        instrument.set_param(param::SOURCE, 1.0);
        instrument.set_param(param::PLAYBACK, 2.0);
        instrument.reset();
        instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
        let mut rendered = Vec::new();
        for (source, frames) in [(1.0, 1024), (0.0, 4096), (1.0, 1024)] {
            instrument.set_param(param::SOURCE, source);
            let mut section = vec![0.0; frames * 2];
            for block in section.chunks_mut(block_frames * 2) {
                instrument.process(&ProcessContext::default(), block);
            }
            rendered.extend(section);
        }
        assert!(rendered[10240..].iter().all(|value| *value == 0.0), "switching back needs a new note");
        rendered
    }
    assert_eq!(performance(17), performance(512));
}

#[test]
fn retiring_a_hidden_source_preserves_the_current_pitch_wheel_for_the_next_note() {
    let mut instrument = Instrument::new();
    let source: std::sync::Arc<[f32]> = (0..4800).map(|i| (i as f32 * 0.04).sin() * 0.2).collect();
    instrument.samples().unwrap().load_sample(48000, 1, source);
    instrument.set_param(param::SOURCE, 1.0);
    instrument.set_param(param::PLAYBACK, 2.0);
    instrument.reset();
    instrument.event(NoteEvent::PitchBend(1.0));
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut before = [0.0; 2048];
    instrument.process(&ProcessContext::default(), &mut before);
    instrument.set_param(param::SOURCE, 0.0);
    let mut silent = [0.0; 2048];
    instrument.process(&ProcessContext::default(), &mut silent);
    instrument.set_param(param::SOURCE, 1.0);
    instrument.process(&ProcessContext::default(), &mut silent);
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut after = [0.0; 2048];
    instrument.process(&ProcessContext::default(), &mut after);
    assert_eq!(after, before);
}

#[test]
fn hybrid_blend_endpoints_advance_held_and_released_layers() {
    for released in [false, true] {
        let mut hybrid = Instrument::new();
        let mut sample = Instrument::new();
        for instrument in [&mut hybrid, &mut sample] {
            let source: std::sync::Arc<[f32]> = (0..4800).map(|i| (i as f32 * 0.04).sin() * 0.2).collect();
            instrument.samples().unwrap().load_sample(48000, 1, source);
            instrument.set_param(param::SOURCE, 1.0);
            instrument.set_param(param::PLAYBACK, 2.0);
            instrument.set_param(param::RELEASE, 0.005);
        }
        hybrid.set_param(param::SOURCE, 2.0);
        hybrid.set_param(param::BLEND, 0.0);
        for instrument in [&mut hybrid, &mut sample] {
            instrument.reset();
            instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
            let mut silent = [0.0; 4096];
            instrument.process(&ProcessContext::default(), &mut silent);
            if released {
                instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
            }
            instrument.process(&ProcessContext::default(), &mut silent);
        }
        hybrid.set_param(param::BLEND, 1.0);
        let mut got = [0.0; 4096];
        let mut reference = [0.0; 4096];
        hybrid.process(&ProcessContext::default(), &mut got);
        sample.process(&ProcessContext::default(), &mut reference);
        assert_eq!(&got[1024..], &reference[1024..], "the blend reveals current sample phase or completed release");
        assert_eq!(got[1024..].iter().all(|value| *value == 0.0), released);
    }
}

#[test]
fn finite_velocities_are_normalized_before_both_source_layers_receive_them() {
    for source in [0.0, 1.0, 2.0] {
        let mut normal = Instrument::new();
        let mut hostile = Instrument::new();
        for instrument in [&mut normal, &mut hostile] {
            instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
            instrument.set_param(param::SOURCE, source);
            instrument.reset();
        }
        normal.event(NoteEvent::On { key: 60, velocity: 1.0 });
        hostile.event(NoteEvent::On { key: 60, velocity: 10.0 });
        let mut expected = [0.0; 1024];
        let mut actual = [0.0; 1024];
        normal.process(&ProcessContext::default(), &mut expected);
        hostile.process(&ProcessContext::default(), &mut actual);
        assert_eq!(actual, expected, "source {source}");
        normal.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        hostile.event(NoteEvent::On { key: 60, velocity: -2.0 });
        normal.process(&ProcessContext::default(), &mut expected);
        hostile.process(&ProcessContext::default(), &mut actual);
        assert_eq!(actual, expected, "nonpositive velocity releases source {source}");
    }
}

#[test]
fn hostile_sample_levels_remain_finite_after_the_final_gain() {
    for source in [1.0, 2.0] {
        for level in [f32::MAX, -f32::MAX, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut instrument = Instrument::new();
            instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![level; 4800]));
            instrument.set_param(param::SOURCE, source);
            instrument.set_param(param::GAIN, 12.0);
            instrument.set_param(param::ATTACK, 0.0);
            instrument.reset();
            instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
            let mut rendered = [0.0; 2048];
            instrument.process(&ProcessContext::default(), &mut rendered);
            assert!(rendered.iter().all(|value| value.is_finite()), "source {source}, level {level}");
            if level.is_finite() {
                assert!(rendered[1024..].iter().all(|value| *value == level.signum() * f32::MAX));
            }
        }
    }
}

#[test]
fn pedal_up_retires_old_same_key_samples_but_preserves_the_held_retrigger() {
    for source in [1.0, 2.0] {
        let mut instrument = Instrument::new();
        let mut reference = Instrument::new();
        for core in [&mut instrument, &mut reference] {
            core.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
            core.set_param(param::SOURCE, source);
            core.set_param(param::PLAYBACK, 2.0);
            core.set_param(param::RELEASE, 0.005);
            core.reset();
        }
        instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
        for _ in 0..3 {
            instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
            instrument.process(&ProcessContext::default(), &mut [0.0; 256]);
            reference.process(&ProcessContext::default(), &mut [0.0; 256]);
            instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        }
        for core in [&mut instrument, &mut reference] {
            core.event(NoteEvent::On { key: 60, velocity: 0.8 });
            core.process(&ProcessContext::default(), &mut [0.0; 256]);
            core.event(NoteEvent::Controller { number: 64, value: 0.0 });
        }
        let mut actual = [0.0; 2048];
        let mut expected = [0.0; 2048];
        instrument.process(&ProcessContext::default(), &mut actual);
        reference.process(&ProcessContext::default(), &mut expected);
        assert!(expected[1024..].iter().any(|value| value.abs() > 0.001));
        assert!(actual[1024..] == expected[1024..], "source {source}: only the held retrigger may remain");
        instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        instrument.process(&ProcessContext::default(), &mut actual);
        assert!(actual[1024..].iter().all(|value| *value == 0.0));
    }
}

#[test]
fn retiring_a_hidden_sample_source_preserves_the_pedal_for_the_next_note() {
    let mut instrument = Instrument::new();
    instrument.samples().unwrap().load_sample(48000, 1, std::sync::Arc::from(vec![0.2; 4800]));
    instrument.set_param(param::SOURCE, 1.0);
    instrument.set_param(param::PLAYBACK, 2.0);
    instrument.set_param(param::RELEASE, 0.005);
    instrument.reset();
    instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
    let mut block = [0.0; 2048];
    for source in [1.0, 0.0, 1.0] {
        instrument.set_param(param::SOURCE, source);
        instrument.process(&ProcessContext::default(), &mut block);
    }
    instrument.event(NoteEvent::On { key: 60, velocity: 1.0 });
    instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    instrument.process(&ProcessContext::default(), &mut block);
    assert!(block[1024..].iter().any(|value| value.abs() > 0.01));
    instrument.event(NoteEvent::Controller { number: 64, value: 0.0 });
    instrument.process(&ProcessContext::default(), &mut block);
    assert!(block[1024..].iter().all(|value| *value == 0.0));
}

fn bar_instrument(body: f64, model: f64) -> Instrument {
    let mut instrument = Instrument::new();
    instrument.set_param(param::REVISION, 3.0);
    instrument.set_param(param::BODY, body);
    instrument.set_param(param::MALLET_MODEL, model);
    instrument.reset();
    instrument
}

fn active_voices(instrument: &Instrument) -> usize {
    instrument.voices.iter().filter(|voice| voice.body.is_active()).count()
}

#[test]
fn revision_three_struck_is_revision_one_and_a_stored_one_reads_as_three() {
    let render = |revision: f64, body: f64| {
        let mut instrument = Instrument::new();
        instrument.set_param(param::REVISION, revision);
        instrument.set_param(param::BODY, body);
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        let mut buffer = vec![0.0; 16384];
        instrument.process(&ProcessContext::default(), &mut buffer);
        buffer
    };
    assert_eq!(render(3.0, 0.0), render(1.0, 0.0), "the Struck body is the revision 1 renderer");
    for body in [1.0, 2.0] {
        let bar = render(3.0, body);
        assert_eq!(render(1.0, body), bar, "a stored 1 is read as revision 3");
        assert_ne!(bar, render(3.0, 0.0));
        assert!(render(2.0, body).iter().all(|x| *x == 0.0), "revision 2 needs an accepted profile");
    }
    assert_eq!(PARAMS[param::REVISION].default, 1.0, "a project without a stored revision is revision 1");
    assert_eq!(MODEL_REVISION, 3, "a new Instrument stores revision 3");
}

#[test]
fn a_roll_strikes_one_bar_and_keeps_its_motion() {
    for (body, model) in [(1.0, 0.0), (1.0, 1.0), (2.0, 0.0), (2.0, 1.0)] {
        let mut instrument = bar_instrument(body, model);
        instrument.set_param(param::DECAY, 20.0);
        let mut block = [0.0; 256];
        for key in 40..55 {
            instrument.event(NoteEvent::On { key, velocity: 0.7 });
        }
        for _ in 0..64 {
            instrument.event(NoteEvent::On { key: 72, velocity: 0.9 });
            instrument.process(&ProcessContext::default(), &mut block);
            instrument.event(NoteEvent::Off { key: 72, velocity: 0.0 });
            instrument.process(&ProcessContext::default(), &mut block);
        }
        assert_eq!(instrument.voices.iter().filter(|voice| voice.key == 72).count(), 1, "body {body}: one rolled bar");
        let held =
            instrument.voices.iter().filter(|voice| (40..55).contains(&voice.key) && voice.body.is_active()).count();
        assert_eq!(held, 15, "body {body}: the roll steals none of the other bars");
    }
}

#[test]
fn a_new_body_on_a_ringing_key_takes_another_voice() {
    let mut instrument = bar_instrument(1.0, 0.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    instrument.set_param(param::BODY, 2.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    instrument.set_param(param::BODY, 0.0);
    instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
    let mut block = [0.0; 64];
    instrument.process(&ProcessContext::default(), &mut block);
    assert_eq!(active_voices(&instrument), 3);
}

#[test]
fn marimba_rings_through_note_off_and_vibraphone_damps_unless_the_pedal_holds() {
    let tail = |body: f64, pedal: bool| {
        let mut instrument = bar_instrument(body, 0.0);
        instrument.set_param(param::DECAY, 20.0);
        instrument.set_param(param::RELEASE, 0.01);
        if pedal {
            instrument.event(NoteEvent::Controller { number: 64, value: 1.0 });
        }
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        let mut block = vec![0.0; 9600];
        instrument.process(&ProcessContext::default(), &mut block);
        let late = block[8000..].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        (instrument, late)
    };
    assert!(tail(1.0, false).1 > 0.01, "marimba rings freely");
    assert!(tail(2.0, false).1 < 1e-4, "vibraphone is damped on note-off");
    let (mut held, late) = tail(2.0, true);
    assert!(late > 0.01, "the pedal holds the vibraphone");
    held.event(NoteEvent::Controller { number: 64, value: 0.0 });
    let mut block = vec![0.0; 9600];
    held.process(&ProcessContext::default(), &mut block);
    assert!(block[8000..].iter().all(|x| x.abs() < 1e-4), "lifting the pedal damps released bars");
    let (mut marimba, _) = tail(1.0, false);
    marimba.event(NoteEvent::AllNotesOff);
    marimba.process(&ProcessContext::default(), &mut block);
    assert!(block[8000..].iter().all(|x| x.abs() < 1e-4), "all notes off damps every bar");
}

#[test]
fn bar_performances_are_identical_across_block_partitions() {
    for (body, model) in [(1.0, 0.0), (1.0, 1.0), (2.0, 0.0), (2.0, 1.0)] {
        let performance = |block_frames: usize| {
            let mut instrument = bar_instrument(body, model);
            let mut rendered = Vec::new();
            for step in 0..48 {
                match step % 6 {
                    0 => instrument
                        .event(NoteEvent::On { key: 60 + (step % 5) as u8, velocity: 0.3 + step as f32 / 80.0 }),
                    1 => instrument.event(NoteEvent::On { key: 60, velocity: 1.0 }),
                    2 => instrument.event(NoteEvent::PitchBend((step as f32 / 48.0) - 0.5)),
                    3 => instrument.event(NoteEvent::Controller { number: 64, value: (step % 12) as f32 / 6.0 }),
                    4 => instrument.event(NoteEvent::Off { key: 60, velocity: 0.0 }),
                    _ => instrument.set_param(param::HARDNESS, step as f64 / 48.0),
                }
                // 37 frames per event: contacts span block boundaries at every partition.
                let mut section = vec![0.0; 37 * 2];
                for block in section.chunks_mut(block_frames * 2) {
                    instrument.process(&ProcessContext::default(), block);
                }
                rendered.extend(section);
            }
            rendered
        };
        let whole = performance(37);
        assert!(whole.iter().any(|x| x.abs() > 0.01));
        assert_eq!(performance(1), whole, "body {body}, model {model}");
        assert_eq!(performance(5), whole, "body {body}, model {model}");
    }
}

#[test]
fn hostile_bar_settings_and_events_stay_finite() {
    let mut instrument = bar_instrument(1.0, 0.0);
    for index in [
        param::BODY,
        param::HARDNESS,
        param::MALLET_MODEL,
        param::RESONATOR,
        param::STRIKE,
        param::DECAY,
        param::QUALITY,
    ] {
        for value in [f64::NAN, f64::INFINITY, -1e300, 1e300] {
            instrument.set_param(index, value);
        }
    }
    for velocity in [f32::MIN_POSITIVE, 1e-30, 1.0, f32::MAX, f32::NAN] {
        for key in [0, 1, 60, 126, 127, 200] {
            instrument.event(NoteEvent::On { key, velocity });
        }
    }
    let mut block = [0.0; 4096];
    instrument.process(&ProcessContext::default(), &mut block);
    assert!(block.iter().all(|x| x.is_finite() && x.abs() < 16.0));
    assert!(std::mem::size_of::<Voice>() <= 15 * 1024, "{}", std::mem::size_of::<Voice>());
    assert!(std::mem::size_of::<Instrument>() <= 256 * 1024);
}

/// Self-authored regression anchors of both profiles and both mallet models, written from this
/// implementation (`SUHINA_BLESS=1`), not an independent reference.
#[test]
fn bar_bodies_match_their_render_anchors() {
    const FRAMES: [usize; 16] = [0, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 377, 987, 2584, 6765];
    let mut text = String::from(
        "# CC0-1.0: self-authored regression anchors of Instrument revision 3 bar bodies, written by\n\
         # this implementation (SUHINA_BLESS=1 cargo test -p suhina-device-instrument anchors).\n\
         # Body, mallet model, frame, left channel; defaults, key 60, velocity 0.8, 48000 Hz.\n",
    );
    let mut rendered = Vec::new();
    for (body, name) in [(1.0, "marimba"), (2.0, "vibraphone")] {
        for (model, model_name) in [(0.0, "contact"), (1.0, "pulse")] {
            let mut instrument = bar_instrument(body, model);
            instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
            let mut buffer = vec![0.0; 8192 * 2];
            instrument.process(&ProcessContext::default(), &mut buffer);
            for frame in FRAMES {
                text.push_str(&format!("{name} {model_name} {frame} {:.9e}\n", buffer[frame * 2]));
                rendered.push((name, model_name, frame, buffer[frame * 2]));
                assert_eq!(buffer[frame * 2], buffer[frame * 2 + 1]);
            }
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/modal-v2-bar-anchors.txt");
    if suhina_test_support::blessing() {
        std::fs::write(&path, text).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(&path).unwrap();
    let anchors: Vec<_> = stored.lines().filter(|line| !line.starts_with('#')).collect();
    // On a difference, the whole file as this run renders it, to compare or to record from CI.
    let report = |what: String| {
        format!(
            "{what}

The file as this run renders it, between the lines:
----- {} -----
{text}----- end -----",
            path.display()
        )
    };
    assert_eq!(anchors.len(), rendered.len(), "{}", report("the anchor count differs".into()));
    for (line, (name, model, frame, value)) in anchors.iter().zip(&rendered) {
        let fields: Vec<_> = line.split(' ').collect();
        assert_eq!(fields[..3], [*name, *model, frame.to_string().as_str()], "{}", report("the anchors differ".into()));
        let expected: f32 = fields[3].parse().unwrap();
        assert!(
            (value - expected).abs() <= 2e-6 + expected.abs() * 1e-5,
            "{}",
            report(format!("{name} {model} frame {frame}: {value} vs {expected}"))
        );
    }
}

#[test]
fn bars_spread_across_the_stereo_image_by_key_and_struck_stays_centerd() {
    let render = |body: f64, key: u8, width: f64| {
        let mut instrument = bar_instrument(body, 0.0);
        instrument.set_param(param::STEREO_WIDTH, width);
        instrument.event(NoteEvent::On { key, velocity: 0.8 });
        let mut buffer = vec![0.0; 9600];
        instrument.process(&ProcessContext::default(), &mut buffer);
        let energy =
            |channel: usize| buffer.iter().skip(channel).step_by(2).map(|x| f64::from(*x).powi(2)).sum::<f64>();
        (energy(0), energy(1), buffer)
    };
    let (left, right, _) = render(1.0, 40, 1.0);
    assert!(left > right * 4.0, "a low bar sits to the left");
    let (left, right, _) = render(1.0, 84, 1.0);
    assert!(right > left * 4.0, "a high bar sits to the right");
    for (body, key, width) in [(1.0, 40, 0.0), (1.0, 60, 1.0), (0.0, 40, 1.0), (2.0, 60, 1.0)] {
        let (_, _, buffer) = render(body, key, width);
        assert!(buffer.chunks(2).all(|frame| frame[0] == frame[1]), "body {body}, key {key}, width {width}");
    }
}

#[test]
fn low_bars_get_a_softer_mallet_from_a_graded_set() {
    assert_eq!(bars::graded_hardness(60, 0.5), 0.5);
    assert_eq!(bars::graded_hardness(96, 0.5), 0.5);
    assert!((bars::graded_hardness(42, 0.5) - 0.35).abs() < 1e-12);
    assert!((bars::graded_hardness(24, 0.5) - 0.2).abs() < 1e-12);
    assert_eq!(bars::graded_hardness(0, 0.1), 0.0);
    assert_eq!(bars::graded_hardness(30, f64::NAN), 0.0);
}

#[test]
fn every_body_sounds_finite_and_the_kalimba_plucks_from_rest() {
    let peak = |buffer: &[f32]| buffer.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    for body in 1..=5 {
        for key in [48, 72, 96] {
            let mut instrument = bar_instrument(f64::from(body), 0.0);
            instrument.event(NoteEvent::On { key, velocity: 0.8 });
            let mut buffer = vec![0.0; 9600];
            instrument.process(&ProcessContext::default(), &mut buffer);
            assert!(buffer.iter().all(|x| x.is_finite()), "body {body}, key {key}");
            assert!(peak(&buffer) > 1e-4, "body {body}, key {key} sounds");
            assert!(peak(&buffer) < 1.0, "body {body}, key {key}: {}", peak(&buffer));
        }
    }
    let mut kalimba = bar_instrument(5.0, 0.0);
    kalimba.event(NoteEvent::On { key: 60, velocity: 1.0 });
    let mut first = [0.0; 2];
    kalimba.process(&ProcessContext::default(), &mut first);
    assert_eq!(first, [0.0, 0.0], "a plucked tine starts at rest, without a click");
}

#[test]
fn shaping_controls_change_a_bar_and_leave_struck_alone() {
    let render = |body: f64, index: usize, value: f64| {
        let mut instrument = bar_instrument(body, 0.0);
        instrument.set_param(index, value);
        instrument.event(NoteEvent::On { key: 60, velocity: 0.8 });
        let mut buffer = vec![0.0; 9600];
        instrument.process(&ProcessContext::default(), &mut buffer);
        buffer
    };
    for (index, value) in [(param::TUNING, 0.0), (param::RING, 1.0), (param::TORSION, 1.0), (param::MALLET_WEIGHT, 1.0)]
    {
        let default = PARAMS[index].default;
        assert_ne!(render(1.0, index, value), render(1.0, index, default), "{} shapes a bar", PARAMS[index].id);
        assert_eq!(render(0.0, index, value), render(0.0, index, default), "{} leaves Struck alone", PARAMS[index].id);
    }
}

#[test]
fn the_panel_reads_each_modes_energy_the_time_since_a_strike_and_its_velocity() {
    let read = |instrument: &Instrument| instrument.readouts().unwrap();
    let mut marimba = bar_instrument(1.0, 0.0);
    let mut buffer = vec![0.0; 960];
    marimba.process(&ProcessContext::default(), &mut buffer);
    let rest = read(&marimba);
    assert_eq!(&rest[..5], &[0.0; 5], "nothing sounds yet");
    assert_eq!(rest[5], 60.0, "no note-on yet");
    assert_eq!(rest[6], 0.0);

    marimba.event(NoteEvent::On { key: 60, velocity: 0.8 });
    marimba.process(&ProcessContext::default(), &mut buffer);
    let struck = read(&marimba);
    assert!(struck[0] > 0.1, "the fundamental sounds: {:?}", &struck[..5]);
    assert!(
        struck[..5].iter().all(|e| (0.0..=0.8).contains(e)),
        "energies stay within the velocity: {:?}",
        &struck[..5]
    );
    assert!((struck[5] - 0.01).abs() < 1e-4, "480 frames at 48 kHz since the note-on: {}", struck[5]);
    assert_eq!(struck[6], 0.8);

    // Two seconds later the upper modes have died away faster than the fundamental.
    let mut long = vec![0.0; 192_000];
    marimba.process(&ProcessContext::default(), &mut long);
    let later = read(&marimba);
    assert!(later[0] < struck[0], "the fundamental decays");
    assert!(
        later[1] < 0.5 * struck[1].max(1e-6) || later[1] < 1e-3,
        "the second mode decays faster: {:?} then {:?}",
        &struck[..5],
        &later[..5]
    );
    assert!((later[5] - 2.01).abs() < 1e-3, "{}", later[5]);

    // Struck and a Sample-only source.
    let mut struck_body = bar_instrument(0.0, 0.0);
    struck_body.event(NoteEvent::On { key: 60, velocity: 1.0 });
    struck_body.process(&ProcessContext::default(), &mut buffer);
    assert!(read(&struck_body)[0] > 0.0, "Struck hands its first resonance's energy");
    marimba.reset();
    let reset = read(&marimba);
    assert_eq!(&reset[..5], &[0.0; 5]);
    assert_eq!(reset[5], 60.0);
}
