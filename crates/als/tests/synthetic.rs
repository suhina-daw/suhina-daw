// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests against small Live Set XML documents written by hand from `docs/spec/als-format.md`.

use std::io::Write;

use suhina_als::{AlsError, read_gzip, read_xml};
use suhina_model::{ChannelRole, LaneData, LoopRange, TempoPoint, TimeSignature, TimeSignatureChange};

fn mixer(volume: &str, pan: &str, on: &str, sends: &[&str]) -> String {
    let sends: String = sends
        .iter()
        .enumerate()
        .map(|(i, v)| format!(r#"<TrackSendHolder Id="{i}"><Send><Manual Value="{v}"/></Send></TrackSendHolder>"#))
        .collect();
    format!(
        r#"<Mixer><Sends>{sends}</Sends><Speaker><Manual Value="{on}"/></Speaker>
           <Pan><Manual Value="{pan}"/></Pan><Volume><Manual Value="{volume}"/></Volume></Mixer>"#
    )
}

fn midi_clip(time: &str, start: &str, end: &str, loop_on: &str, loop_end: &str, notes: &str) -> String {
    format!(
        r#"<MidiClip Id="0" Time="{time}">
             <CurrentStart Value="{start}"/><CurrentEnd Value="{end}"/>
             <Loop><LoopStart Value="0"/><LoopEnd Value="{loop_end}"/><StartRelative Value="0"/>
                   <LoopOn Value="{loop_on}"/><OutMarker Value="{loop_end}"/></Loop>
             <Name Value="Clip"/><Disabled Value="false"/>
             <Notes><KeyTracks>{notes}</KeyTracks></Notes>
           </MidiClip>"#
    )
}

fn key_track(key: u8, events: &str) -> String {
    format!(r#"<KeyTrack Id="0"><Notes>{events}</Notes><MidiKey Value="{key}"/></KeyTrack>"#)
}

/// Two MIDI tracks, one audio track, one return; two scenes.
fn live_set() -> String {
    let note = |t: &str, d: &str, v: &str| {
        format!(r#"<MidiNoteEvent Time="{t}" Duration="{d}" Velocity="{v}" OffVelocity="64" NoteId="1"/>"#)
    };
    let arrangement_clip = midi_clip("8", "8", "12", "false", "4", &key_track(64, &note("1", "2", "63.5")));
    let session_clip = midi_clip("0", "0", "8", "true", "8", &key_track(60, &note("0", "1", "127")));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5" MinorVersion="12.0_12402" Creator="Ableton Live 12.4.6">
  <LiveSet>
    <Tracks>
      <MidiTrack Id="12">
        <Name><EffectiveName Value="1-MIDI"/></Name><TrackGroupId Value="-1"/>
        <DeviceChain>
          <AudioOutputRouting><Target Value="AudioOut/Main"/></AudioOutputRouting>
          {mixer_midi}
          <MainSequencer>
            <ClipSlotList>
              <ClipSlot Id="0"><ClipSlot><Value>{session_clip}</Value></ClipSlot><HasStop Value="true"/></ClipSlot>
              <ClipSlot Id="1"><ClipSlot><Value/></ClipSlot><HasStop Value="false"/></ClipSlot>
            </ClipSlotList>
            <ClipTimeable><ArrangerAutomation><Events>{arrangement_clip}</Events></ArrangerAutomation></ClipTimeable>
          </MainSequencer>
          <DeviceChain><Devices/></DeviceChain>
        </DeviceChain>
      </MidiTrack>
      <AudioTrack Id="8">
        <Name><EffectiveName Value="3-Audio"/></Name><TrackGroupId Value="-1"/>
        <DeviceChain>
          {mixer_audio}
          <MainSequencer>
            <ClipSlotList>
              <ClipSlot Id="0"><ClipSlot><Value><AudioClip Id="0" Time="0"/></Value></ClipSlot><HasStop Value="true"/></ClipSlot>
              <ClipSlot Id="1"><ClipSlot><Value/></ClipSlot><HasStop Value="true"/></ClipSlot>
            </ClipSlotList>
          </MainSequencer>
        </DeviceChain>
      </AudioTrack>
      <ReturnTrack Id="2">
        <Name><EffectiveName Value="A-Reverb"/></Name><TrackGroupId Value="-1"/>
        <DeviceChain>{mixer_return}<DeviceChain><Devices><Eq8 Id="0"/></Devices></DeviceChain></DeviceChain>
      </ReturnTrack>
    </Tracks>
    <MainTrack>
      <DeviceChain><Mixer>
        <Speaker><Manual Value="true"/></Speaker><Pan><Manual Value="0"/></Pan><Volume><Manual Value="1"/></Volume>
        <Tempo><Manual Value="128"/></Tempo><TimeSignature><Manual Value="201"/></TimeSignature>
      </Mixer></DeviceChain>
    </MainTrack>
    <Scenes>
      <Scene Id="0"><Name Value="Intro"/><Tempo Value="90"/><IsTempoEnabled Value="true"/></Scene>
      <Scene Id="5"><Name Value=""/><Tempo Value="120"/><IsTempoEnabled Value="false"/></Scene>
    </Scenes>
  </LiveSet>
</Ableton>"#,
        mixer_midi = mixer("1", "0", "true", &["0.0003162277571"]),
        mixer_audio = mixer("0.5011872053", "-1", "false", &["0.2511887252"]),
        mixer_return = mixer("1", "0", "true", &["0.0003162277571"]),
    )
}

#[test]
fn reads_transport_tracks_and_mixer() {
    let import = read_xml(&live_set()).unwrap();
    let p = &import.project;
    assert_eq!(p.transport.tempo_bpm, 128.0);
    assert_eq!(p.tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["1-MIDI", "3-Audio", "A-Reverb"]);

    let main = p.channels.iter().find(|c| c.role == ChannelRole::Master).unwrap();
    assert_eq!(main.volume, 1.0);

    let audio = p.channels.iter().find(|c| c.name == "3-Audio").unwrap();
    assert!((audio.volume - 0.501).abs() < 1e-3);
    assert_eq!(audio.pan, -1.0);
    assert!(audio.mute);
    let ret = p.channels.iter().find(|c| c.name == "A-Reverb").unwrap();
    assert_eq!(ret.role, ChannelRole::Effect);
    assert_eq!(audio.sends.len(), 1);
    assert_eq!(audio.sends[0].target, ret.id);
    assert!((audio.sends[0].level - 0.251).abs() < 1e-3);

    // A send at -70 dB ("-inf" in Live) is no send.
    let midi = p.channels.iter().find(|c| c.name == "1-MIDI").unwrap();
    assert!(midi.sends.is_empty());
    assert!(!midi.mute);
}

#[test]
fn reads_arrangement_clip_and_notes() {
    let p = read_xml(&live_set()).unwrap().project;
    let lane = p.arrangement.lanes.iter().find(|l| l.name == "1-MIDI").unwrap();
    let clip = &lane.clips[0];
    assert_eq!((clip.start, clip.length, clip.offset), (8.0, 4.0, 0.0));
    assert_eq!(clip.loop_range, None);

    let content = p.content(clip.content).unwrap();
    let LaneData::Notes(notes) = &content.lanes[0].data else { panic!("expected notes") };
    assert_eq!(notes.len(), 1);
    assert_eq!((notes[0].key, notes[0].time, notes[0].duration), (64, 1.0, 2.0));
    assert!((notes[0].velocity - 0.5).abs() < 1e-6);
}

/// A clip with one enabled note and one deactivated note (`IsEnabled="false"`): the deactivated
/// note is kept and read as muted, not dropped (`docs/spec/als-format.md` › Notes).
#[test]
fn a_deactivated_note_is_read_as_muted() {
    let events = concat!(
        r#"<MidiNoteEvent Time="0" Duration="1" Velocity="100" OffVelocity="64" NoteId="1"/>"#,
        r#"<MidiNoteEvent Time="1" Duration="1" Velocity="100" OffVelocity="64" NoteId="2" IsEnabled="false"/>"#,
    );
    let clip = midi_clip("0", "0", "4", "false", "4", &key_track(60, events));
    let xml = format!(
        r#"<Ableton><LiveSet><Tracks><MidiTrack Id="1"><Name><EffectiveName Value="Keys"/></Name>
           <DeviceChain><MainSequencer><ClipSlotList>
             <ClipSlot Id="0"><ClipSlot><Value>{clip}</Value></ClipSlot><HasStop Value="true"/></ClipSlot>
           </ClipSlotList></MainSequencer></DeviceChain></MidiTrack></Tracks></LiveSet></Ableton>"#
    );
    let p = read_xml(&xml).unwrap().project;
    let content = p.contents.first().expect("the clip's content");
    let LaneData::Notes(notes) = &content.lanes[0].data else { panic!("expected notes") };
    assert_eq!(notes.len(), 2, "the deactivated note is kept, not dropped");
    assert_eq!((notes[0].muted, notes[1].muted), (false, true));
    assert_eq!((notes[1].time, notes[1].duration, notes[1].key), (1.0, 1.0, 60));
}

#[test]
fn reads_scenes_and_session_slots() {
    let p = read_xml(&live_set()).unwrap().project;
    assert_eq!(p.launcher.scenes.len(), 2);
    assert_eq!(p.launcher.scenes[0].name, "Intro");
    assert_eq!(p.launcher.scenes[0].tempo_bpm, Some(90.0));
    assert_eq!(p.launcher.scenes[1].tempo_bpm, None);

    // Columns only for MIDI and audio tracks, one slot per scene.
    assert_eq!(p.launcher.columns.len(), 2);
    let midi = &p.launcher.columns[0];
    assert_eq!(midi.slots.len(), 2);
    let clip = midi.slots[0].clip.as_ref().unwrap();
    assert_eq!(clip.loop_range.map(|r| (r.start, r.end)), Some((0.0, 8.0)));
    assert_eq!(clip.length, 8.0);
    assert!(midi.slots[0].has_stop);
    assert!(midi.slots[1].clip.is_none());
    assert!(!midi.slots[1].has_stop);
}

#[test]
fn unsupported_content_becomes_warnings() {
    let import = read_xml(&live_set()).unwrap();
    assert!(import.warnings.iter().any(|w| w.contains("has no file")), "{:?}", import.warnings);
    assert!(import.warnings.iter().any(|w| w.contains("Eq8")), "{:?}", import.warnings);
    assert!(import.project.launcher.columns[1].slots[0].clip.is_none());
}

#[test]
fn reads_gzip() {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(live_set().as_bytes()).unwrap();
    let bytes = encoder.finish().unwrap();
    let import = read_gzip(bytes.as_slice()).unwrap();
    assert_eq!(import.project.transport.tempo_bpm, 128.0);
}

#[test]
fn rejects_files_that_are_not_live_sets() {
    assert!(matches!(read_gzip(&b"not gzip at all"[..]), Err(AlsError::Io(_))));
    assert!(matches!(read_xml("<not xml"), Err(AlsError::Xml(_))));
    assert!(matches!(read_xml("<Other/>"), Err(AlsError::NotALiveSet)));
    assert!(matches!(read_xml("<Ableton/>"), Err(AlsError::NotALiveSet)));
}

#[test]
fn hostile_values_do_not_panic() {
    let xml = live_set()
        .replace(r#"<Tempo><Manual Value="128"/>"#, r#"<Tempo><Manual Value="NaN"/>"#)
        .replace(r#"Velocity="63.5""#, r#"Velocity="1e400""#)
        .replace(r#"<MidiKey Value="64"/>"#, r#"<MidiKey Value="300"/>"#)
        .replace(r#"<TimeSignature><Manual Value="201"/>"#, r#"<TimeSignature><Manual Value="999"/>"#)
        .replace(r#"<CurrentEnd Value="12"/>"#, r#"<CurrentEnd Value="-5"/>"#);
    let import = read_xml(&xml).unwrap();
    assert_eq!(import.project.transport.tempo_bpm, 120.0);
    assert!(import.warnings.iter().any(|w| w.contains("tempo")));
    assert!(import.warnings.iter().any(|w| w.contains("time signature")));
    assert!(import.warnings.iter().any(|w| w.contains("no length")));

    let empty = read_xml(r#"<Ableton><LiveSet/></Ableton>"#).unwrap();
    assert!(empty.project.tracks.is_empty());
}

#[test]
fn reads_plugins_and_placeholders() {
    use suhina_model::DeviceKind;
    let devices = r#"
        <PluginDevice Id="0"><On><Manual Value="true"/></On><UserName Value=""/>
          <PluginDesc><VstPluginInfo Id="0">
            <Path Value="C:/Plugins/Thing.dll"/><PlugName Value="Thing"/><UniqueId Value="-2"/>
            <Preset><VstPreset Id="0"><Type Value="1178747752"/><ProgramCount Value="1"/><ParameterCount Value="4"/>
              <ProgramNumber Value="0"/><PluginVersion Value="5"/><ByteOrder Value="2"/><Buffer>0A0b
              FF</Buffer></VstPreset></Preset>
          </VstPluginInfo></PluginDesc>
        </PluginDevice>
        <PluginDevice Id="3"><On><Manual Value="true"/></On><UserName Value=""/>
          <PluginDesc><VstPluginInfo Id="0"><PlugName Value="Odd"/><UniqueId Value="9"/>
            <Preset><VstPreset Id="0"><Type Value="12"/><ProgramCount Value="1"/><ParameterCount Value="0"/><Buffer>01</Buffer></VstPreset></Preset>
          </VstPluginInfo></PluginDesc>
        </PluginDevice>
        <PluginDevice Id="1"><On><Manual Value="false"/></On><UserName Value="My Synth"/>
          <PluginDesc><Vst3PluginInfo Id="0">
            <Preset><Vst3Preset Id="0"><ProcessorState>0102</ProcessorState><ControllerState>zz</ControllerState></Vst3Preset></Preset>
            <Name Value="Synth"/>
            <Uid><Fields.0 Value="1448301683"/><Fields.1 Value="-1"/><Fields.2 Value="0"/><Fields.3 Value="16"/></Uid>
          </Vst3PluginInfo></PluginDesc>
        </PluginDevice>
        <Eq8 Id="2"><On><Manual Value="false"/></On><UserName Value=""/></Eq8>"#;
    let xml = format!(
        r#"<Ableton><LiveSet><Tracks><MidiTrack Id="1"><Name><EffectiveName Value="M"/></Name>
           <DeviceChain><DeviceChain><Devices>{devices}</Devices></DeviceChain></DeviceChain></MidiTrack></Tracks></LiveSet></Ableton>"#
    );
    let import = read_xml(&xml).unwrap();
    let channel = import.project.channels.iter().find(|c| c.name == "M").unwrap();
    let [vst2, odd, vst3, eq] = channel.devices.as_slice() else {
        panic!("expected four devices: {:?}", channel.devices)
    };

    assert_eq!(vst2.kind, DeviceKind::Vst2 { id: u32::MAX - 1 });
    assert_eq!(vst2.name, "Thing");
    // The chunk becomes a standard VST2 bank: CcnK, size, FBCh, version 2, id, plug-in version,
    // programs, current program, 124 zero bytes, chunk size, chunk.
    let bank = vst2.state.as_deref().unwrap();
    assert_eq!(&bank[..4], b"CcnK");
    assert_eq!(&bank[8..12], b"FBCh");
    assert_eq!(&bank[16..24], [0xff, 0xff, 0xff, 0xfe, 0, 0, 0, 5]);
    assert_eq!(&bank[156..], [0, 0, 0, 3, 0x0a, 0x0b, 0xff]);
    // State of a kind that cannot be read is kept as it was.
    assert_eq!(odd.state, None);
    assert!(odd.extensions.0.iter().any(|e| e.key == "VstPresetBuffer" && e.data == [1]));
    assert!(import.warnings.iter().any(|w| w.contains("VST2 state not read")));
    assert!(vst2.extensions.0.iter().any(|e| e.key == "VstPath" && e.data == b"C:/Plugins/Thing.dll"));

    assert_eq!(vst3.kind, DeviceKind::Vst3 { id: "56535473FFFFFFFF0000000000000010".into() });
    assert_eq!(vst3.name, "My Synth");
    assert!(!vst3.enabled);
    assert_eq!(vst3.state.as_deref(), Some(&[1, 2][..]));
    assert!(import.warnings.iter().any(|w| w.contains("ControllerState is not valid hex")));

    assert_eq!(eq.kind, DeviceKind::Foreign { format: "als".into(), id: "Eq8".into() });
    assert!(!eq.enabled);
}

/// Locators become arrangement markers in beat order; one without a usable time is skipped with
/// a warning, and a set without locators has no markers (`docs/spec/als-format.md` › Locators).
#[test]
fn reads_locators_as_markers() {
    let set = |locators: &str| format!(r#"<Ableton><LiveSet><Tracks/>{locators}</LiveSet></Ableton>"#);
    let locator = |id: u32, time: &str, name: &str| {
        format!(
            r#"<Locator Id="{id}"><LomId Value="0"/><Time Value="{time}"/><Name Value="{name}"/>
               <Annotation Value=""/><IsSongStart Value="false"/></Locator>"#
        )
    };
    let locators =
        [locator(0, "32", "Chorus"), locator(1, "0", "Intro"), locator(2, "-4", "Early"), locator(3, "x", "")];
    let import = read_xml(&set(&format!("<Locators><Locators>{}</Locators></Locators>", locators.concat()))).unwrap();
    let markers: Vec<(f64, &str)> =
        import.project.arrangement.markers.iter().map(|m| (m.beat, m.name.as_str())).collect();
    assert_eq!(markers, [(0.0, "Intro"), (32.0, "Chorus")]);
    assert_eq!(import.warnings.iter().filter(|w| w.contains("locator")).count(), 2, "{:?}", import.warnings);

    assert!(read_xml(&set("")).unwrap().project.arrangement.markers.is_empty());
    assert!(read_xml(&set("<Locators><Locators/></Locators>")).unwrap().project.arrangement.markers.is_empty());
}

/// The transport's loop region and its on/off are read; a region without a length, or one that
/// does not end after it starts, is skipped with a warning (`docs/spec/als-format.md` ›
/// Arrangement loop).
#[test]
fn reads_the_arrangement_loop() {
    let set = |transport: &str| format!(r#"<Ableton><LiveSet><Tracks/>{transport}</LiveSet></Ableton>"#);
    let transport = |on: &str, start: &str, length: &str| {
        format!(
            r#"<Transport><PhaseNudgeTempo Value="10"/><LoopOn Value="{on}"/><LoopStart Value="{start}"/>
               <LoopLength Value="{length}"/><LoopIsSongStart Value="false"/><CurrentTime Value="0"/></Transport>"#
        )
    };
    let read = |xml: &str| read_xml(&set(xml)).unwrap();

    let on = read(&transport("true", "8", "16")).project.transport;
    assert_eq!((on.loop_range, on.loop_enabled), (Some(LoopRange { start: 8.0, end: 24.0 }), true));
    let off = read(&transport("false", "0", "4")).project.transport;
    assert_eq!((off.loop_range, off.loop_enabled), (Some(LoopRange { start: 0.0, end: 4.0 }), false));

    for bad in [transport("true", "8", "0"), transport("true", "-4", "8"), transport("true", "x", "8")] {
        let import = read(&bad);
        assert_eq!(import.project.transport.loop_range, None, "{bad}");
        assert!(!import.project.transport.loop_enabled);
        assert_eq!(import.warnings.iter().filter(|w| w.contains("loop")).count(), 1, "{:?}", import.warnings);
    }
    let none = read("").project.transport;
    assert_eq!((none.loop_range, none.loop_enabled), (None, false));
}

/// A Live Set of the tracks `(kind, id, name, group id)`, each routed into its group when it has
/// one ("-1" is none).
fn grouped_set(tracks: &[(&str, &str, &str, &str)]) -> String {
    let tracks: String = tracks
        .iter()
        .map(|(kind, id, name, group)| {
            let target = if *group == "-1" { "AudioOut/Main" } else { "AudioOut/GroupTrack" };
            format!(
                r#"<{kind} Id="{id}"><Name><EffectiveName Value="{name}"/></Name><TrackGroupId Value="{group}"/>
                   <DeviceChain><AudioOutputRouting><Target Value="{target}"/></AudioOutputRouting>{mixer}</DeviceChain>
                   </{kind}>"#,
                mixer = mixer("1", "0", "true", &[]),
            )
        })
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5" MinorVersion="12.0_12402" Creator="Ableton Live 12.4.6">
  <LiveSet><Tracks>{tracks}</Tracks>
    <MainTrack><DeviceChain><Mixer><Tempo><Manual Value="120"/></Tempo></Mixer></DeviceChain></MainTrack>
  </LiveSet>
</Ableton>"#
    )
}

/// The live set with tempo and time-signature envelopes on its main track holding `tempo` and
/// `signature` events after their default events (`docs/spec/als-format.md` › Tempo and
/// time-signature changes).
fn with_envelopes(tempo: &str, signature: &str) -> String {
    live_set().replace(
        r#"<Tempo><Manual Value="128"/></Tempo><TimeSignature><Manual Value="201"/></TimeSignature>
      </Mixer></DeviceChain>"#,
        &format!(
            r#"<Tempo><Manual Value="128"/><AutomationTarget Id="8"/></Tempo>
        <TimeSignature><Manual Value="201"/><AutomationTarget Id="9"/></TimeSignature>
      </Mixer></DeviceChain>
      <AutomationEnvelopes><Envelopes>
        <AutomationEnvelope Id="0"><EnvelopeTarget><PointeeId Value="8"/></EnvelopeTarget><Automation><Events>
          <FloatEvent Id="1" Time="-63072000" Value="128"/>{tempo}
        </Events></Automation></AutomationEnvelope>
        <AutomationEnvelope Id="1"><EnvelopeTarget><PointeeId Value="9"/></EnvelopeTarget><Automation><Events>
          <EnumEvent Id="2" Time="-63072000" Value="201"/>{signature}
        </Events></Automation></AutomationEnvelope>
      </Envelopes></AutomationEnvelopes>"#
        ),
    )
}

#[test]
fn reads_groups_inside_groups() {
    let set = grouped_set(&[
        ("GroupTrack", "20", "Band", "-1"),
        ("GroupTrack", "21", "Drums", "20"),
        ("MidiTrack", "22", "Kick", "21"),
        ("AudioTrack", "23", "Vocal", "20"),
        ("MidiTrack", "24", "Pad", "-1"),
    ]);
    let import = read_xml(&set).unwrap();
    let p = &import.project;
    let id = |name: &str| p.tracks.iter().find(|t| t.name == name).unwrap().id;
    let output = |name: &str| {
        let channel = p.tracks.iter().find(|t| t.name == name).unwrap().channel.unwrap();
        let out = p.channels.iter().find(|c| c.id == channel).unwrap().output?;
        p.tracks.iter().find(|t| t.channel == Some(out)).map(|t| t.name.clone())
    };
    assert_eq!(p.ancestors(id("Kick")), [id("Drums"), id("Band")]);
    assert_eq!(p.group_depth(id("Vocal")), Some(1));
    assert_eq!(p.group_depth(id("Pad")), Some(0));
    assert_eq!(output("Kick").as_deref(), Some("Drums"));
    assert_eq!(output("Drums").as_deref(), Some("Band"));
    assert_eq!(output("Band"), None);
    let roles: Vec<ChannelRole> =
        ["Band", "Drums"].iter().map(|n| p.channels.iter().find(|c| c.name == *n).unwrap().role).collect();
    assert_eq!(roles, [ChannelRole::Submix, ChannelRole::Submix]);
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
}

#[test]
fn groups_that_contain_each_other_or_nest_too_deep_still_open() {
    // Two groups inside each other: one of them is taken out, with a warning.
    let set = grouped_set(&[
        ("GroupTrack", "20", "A", "21"),
        ("GroupTrack", "21", "B", "20"),
        ("MidiTrack", "22", "Kick", "21"),
    ]);
    let import = read_xml(&set).unwrap();
    assert_eq!(import.project.validate(), Ok(()));
    assert!(import.warnings.iter().any(|w| w.contains("contain each other")), "{:?}", import.warnings);
    let a = import.project.tracks.iter().find(|t| t.name == "A").unwrap();
    assert_eq!(a.parent, None);
    assert_eq!(import.project.channels.iter().find(|c| c.name == "A").unwrap().output, None);

    // Groups deeper than Suhina nests them: the deepest is taken out.
    let ids: Vec<String> = (0..=suhina_model::MAX_GROUP_DEPTH + 1).map(|i| (100 + i).to_string()).collect();
    let names: Vec<String> = (0..ids.len()).map(|i| format!("G{i}")).collect();
    let tracks: Vec<(&str, &str, &str, &str)> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| ("GroupTrack", id.as_str(), names[i].as_str(), if i == 0 { "-1" } else { ids[i - 1].as_str() }))
        .collect();
    let import = read_xml(&grouped_set(&tracks)).unwrap();
    assert_eq!(import.project.validate(), Ok(()));
    assert!(import.warnings.iter().any(|w| w.contains("nest more than")), "{:?}", import.warnings);
    let last = import.project.tracks.last().unwrap();
    assert_eq!(last.parent, None);
    assert_eq!(
        import.project.group_depth(import.project.tracks[ids.len() - 2].id),
        Some(suhina_model::MAX_GROUP_DEPTH)
    );
}

#[test]
fn reads_tempo_and_time_signature_changes() {
    // 120 at bar 1, a glide to 140 at bar 3, a jump to 90 at bar 5; 3/4 from bar 2, 7/8 from
    // beat 10 (bar 4 in 3/4).
    let xml = with_envelopes(
        r#"<FloatEvent Id="3" Time="0" Value="120"/><FloatEvent Id="4" Time="8" Value="140"/>
           <FloatEvent Id="5" Time="16" Value="140"/><FloatEvent Id="6" Time="16" Value="90"/>"#,
        r#"<EnumEvent Id="7" Time="4" Value="200"/><EnumEvent Id="8" Time="10" Value="303"/>"#,
    );
    let import = read_xml(&xml).unwrap();
    let t = &import.project.transport;
    assert_eq!(t.tempo_bpm, 120.0);
    assert_eq!(
        t.tempo_changes,
        [
            TempoPoint { beat: 8.0, bpm: 140.0, ramp: true },
            TempoPoint { beat: 16.0, bpm: 140.0, ramp: true },
            TempoPoint { beat: 16.0, bpm: 90.0, ramp: false },
        ]
    );
    let ts = |numerator, denominator| TimeSignature { numerator, denominator };
    assert_eq!(
        t.time_signature_changes,
        [
            TimeSignatureChange { bar: 1, time_signature: ts(3, 4) },
            TimeSignatureChange { bar: 3, time_signature: ts(7, 8) },
        ]
    );
    assert!(
        !import.warnings.iter().any(|w| w.contains("tempo") || w.contains("time signature")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn malformed_tempo_and_time_signature_events_are_left_out() {
    let xml = with_envelopes(
        r#"<FloatEvent Id="3" Time="4" Value="NaN"/><FloatEvent Id="4" Time="8" Value="5000"/>
           <FloatEvent Id="5" Time="1e300" Value="100"/><FloatEvent Id="6" Time="x" Value="100"/>
           <FloatEvent Id="7" Time="12" Value="100"/>"#,
        r#"<EnumEvent Id="8" Time="6" Value="200"/><EnumEvent Id="9" Time="8" Value="99999"/>
           <EnumEvent Id="10" Time="16" Value="200"/>"#,
    );
    let import = read_xml(&xml).unwrap();
    let t = &import.project.transport;
    // The one good tempo point is a step: nothing before it is a breakpoint to glide from.
    assert_eq!(t.tempo_changes, [TempoPoint { beat: 12.0, bpm: 100.0, ramp: false }]);
    assert_eq!(t.time_signature_changes.len(), 1);
    assert_eq!(t.time_signature_changes[0].bar, 4);
    assert!(import.warnings.iter().any(|w| w.contains("not on a bar line")), "{:?}", import.warnings);
    assert!(import.warnings.iter().any(|w| w.contains("out of range")), "{:?}", import.warnings);
}
