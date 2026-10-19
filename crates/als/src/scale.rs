// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A MIDI clip's scale (`docs/spec/als-format.md` › MIDI clips › Scale).

use roxmltree::Node;
use suhina_model::{Scale, ScaleMode};

use crate::{boolean, number, value};

/// The clip's Scale Name list in Live's order, with the mode each one is in Suhina. Positions 0
/// (Major), 1 (Minor), 2 (Dorian) and 11 (Minor Pentatonic) are observed; the rest follows the
/// Scale device's list (hypothesis, see the spec). `None`: no Suhina mode holds those notes.
const NAMES: [(&str, Option<ScaleMode>); 35] = [
    ("Major", Some(ScaleMode::Major)),
    ("Minor", Some(ScaleMode::Minor)),
    ("Dorian", Some(ScaleMode::Dorian)),
    ("Mixolydian", Some(ScaleMode::Mixolydian)),
    ("Lydian", Some(ScaleMode::Lydian)),
    ("Phrygian", Some(ScaleMode::Phrygian)),
    ("Locrian", Some(ScaleMode::Locrian)),
    ("Whole Tone", None),
    ("Half-whole Dim.", None),
    ("Whole-half Dim.", None),
    // The minor blues scale: 1, ♭3, 4, ♭5, 5, ♭7, the notes of Suhina's Blues.
    ("Minor Blues", Some(ScaleMode::Blues)),
    ("Minor Pentatonic", Some(ScaleMode::MinorPentatonic)),
    ("Major Pentatonic", Some(ScaleMode::MajorPentatonic)),
    ("Harmonic Minor", Some(ScaleMode::HarmonicMinor)),
    ("Harmonic Major", None),
    ("Dorian #4", None),
    ("Phrygian Dominant", None),
    ("Melodic Minor", Some(ScaleMode::MelodicMinor)),
    ("Lydian Augmented", None),
    ("Lydian Dominant", None),
    ("Super Locrian", None),
    ("8-Tone Spanish", None),
    ("Bhairav", None),
    ("Hungarian Minor", None),
    ("Hirajoshi", None),
    ("In-Sen", None),
    ("Iwato", None),
    ("Kumoi", None),
    ("Pelog Selisir", None),
    ("Pelog Tembung", None),
    ("Messiaen 3", None),
    ("Messiaen 4", None),
    ("Messiaen 5", None),
    ("Messiaen 6", None),
    ("Messiaen 7", None),
];

/// The scale of a `MidiClip`: `Ok(None)` when its Scale Mode is off or not stored, `Err` with
/// the reason when it is on but the stored scale is not one Suhina can hold.
pub(crate) fn clip_scale(clip: Node) -> Result<Option<Scale>, String> {
    if boolean(clip, &["IsInKey"]) != Some(true) {
        return Ok(None);
    }
    // A whole number from 0 up; the casts below cannot cut anything off.
    let index = |name: &str| number(clip, &["ScaleInformation", name]).filter(|n| *n >= 0.0 && n.fract() == 0.0);
    let Some(root) = index("Root").filter(|r| *r <= 11.0) else {
        return Err(match value(clip, &["ScaleInformation", "Root"]) {
            Some(r) => format!("its scale's root {r:?} is not 0 to 11"),
            None => "its scale has no root".into(),
        });
    };
    let Some((name, mode)) = index("Name").and_then(|n| NAMES.get(n as usize)) else {
        return Err(match value(clip, &["ScaleInformation", "Name"]) {
            Some(n) => format!("its scale {n:?} is not one Live lists"),
            None => "its scale has no name".into(),
        });
    };
    let Some(mode) = mode else {
        return Err(format!("Suhina has no {name} scale"));
    };
    Scale::new(root as u8, *mode).map(Some).ok_or_else(|| format!("its scale's root {root} is not 0 to 11"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The scale `clip_scale` reads from a `MidiClip` element holding `inner`.
    fn scale_of(inner: &str) -> Result<Option<Scale>, String> {
        let xml = format!("<MidiClip>{inner}</MidiClip>");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        clip_scale(doc.root_element())
    }

    /// A clip's scale as Live stores it: whether scale mode is on, the root key and the scale name's
    /// number.
    fn stored(in_key: &str, root: &str, name: &str) -> String {
        format!(
            r#"<IsInKey Value="{in_key}"/><ScaleInformation><Root Value="{root}"/><Name Value="{name}"/></ScaleInformation>"#
        )
    }

    /// Each scale name maps to the mode with the notes that name means, and every Suhina mode has
    /// exactly one name.
    #[test]
    fn every_listed_mode_holds_the_notes_of_its_name() {
        let expected: [(&str, &[u8]); 9] = [
            ("Major", &[0, 2, 4, 5, 7, 9, 11]),
            ("Minor", &[0, 2, 3, 5, 7, 8, 10]),
            ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
            ("Mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
            ("Lydian", &[0, 2, 4, 6, 7, 9, 11]),
            ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
            ("Locrian", &[0, 1, 3, 5, 6, 8, 10]),
            ("Minor Blues", &[0, 3, 5, 6, 7, 10]),
            ("Harmonic Minor", &[0, 2, 3, 5, 7, 8, 11]),
        ];
        for (name, semitones) in expected {
            let (_, mode) = NAMES.iter().find(|(n, _)| *n == name).unwrap();
            assert_eq!(mode.unwrap().semitones(), semitones, "{name}");
        }
        // Every Suhina mode is reachable from exactly one name.
        for mode in ScaleMode::ALL {
            assert_eq!(NAMES.iter().filter(|(_, m)| *m == Some(mode)).count(), 1, "{mode:?}");
        }
    }

    /// A stored root and scale number become the same scale in Suhina, from the first name to the
    /// last.
    #[test]
    fn reads_root_and_name() {
        assert_eq!(scale_of(&stored("true", "0", "0")), Ok(Scale::new(0, ScaleMode::Major)));
        assert_eq!(scale_of(&stored("true", "9", "1")), Ok(Scale::new(9, ScaleMode::Minor)));
        assert_eq!(scale_of(&stored("true", "11", "11")), Ok(Scale::new(11, ScaleMode::MinorPentatonic)));
        assert_eq!(scale_of(&stored("true", "2", "17")), Ok(Scale::new(2, ScaleMode::MelodicMinor)));
    }

    /// A clip with scale mode off, a missing scale or a flag that is not `true` has no scale.
    #[test]
    fn scale_mode_off_or_missing_is_no_scale() {
        assert_eq!(scale_of(&stored("false", "2", "2")), Ok(None));
        assert_eq!(scale_of(""), Ok(None));
        assert_eq!(scale_of(&stored("yes", "2", "2")), Ok(None));
    }

    /// Roots and scale numbers out of range, fractional, not finite or too large are refused with a
    /// reason, never a panic.
    #[test]
    fn hostile_values_are_refused_with_a_reason() {
        for (root, name) in [
            ("12", "0"),
            ("-1", "0"),
            ("2.5", "0"),
            ("NaN", "0"),
            ("1e300", "0"),
            ("0", "35"),
            ("0", "-1"),
            ("0", "1.5"),
            ("0", "inf"),
            ("0", "99999999999999999999"),
            ("", ""),
        ] {
            let result = scale_of(&stored("true", root, name));
            assert!(result.is_err(), "{root} {name}: {result:?}");
        }
        assert_eq!(scale_of(&stored("true", "0", "7")), Err("Suhina has no Whole Tone scale".into()));
        assert!(scale_of(r#"<IsInKey Value="true"/>"#).is_err());
    }
}
