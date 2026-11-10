// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn opening_settings_after_a_stream_keeps_all_cached_drivers_without_probing() {
    let mut cached = None;
    let before = cached_listing(&mut cached, || Ok(vec!["first", "selected", "last"])).unwrap();
    let during = cached_listing(&mut cached, || panic!("must not load another driver during playback")).unwrap();
    assert_eq!(before, during);
    assert_eq!(during.iter().find(|name| **name == "selected"), Some(&"selected"));
}

#[test]
fn buffers_are_the_drivers_powers_of_two_and_ends() {
    assert_eq!(allowed_buffers(None), BUFFER_SIZES.to_vec());
    assert_eq!(allowed_buffers(Some((32, 1024))), vec![64, 128, 256, 512, 1024]);
    assert_eq!(allowed_buffers(Some((96, 2000))), vec![96, 128, 256, 512, 1024, 2000]);
    // Nonsense from a driver: as if it said nothing.
    assert_eq!(allowed_buffers(Some((0, 0))), BUFFER_SIZES.to_vec());
    assert_eq!(allowed_buffers(Some((2048, 64))), BUFFER_SIZES.to_vec());
    // A range the engine has no size in holds nothing: no size of it is offered, and none
    // of it is ever asked for.
    assert!(allowed_buffers(Some((8, 32))).is_empty());
    assert!(allowed_buffers(Some((8192, 16384))).is_empty());
}

#[test]
fn the_default_configs_range_is_the_one_reported() {
    // WASAPI gives the device period in frames at each config's rate, lowest first: the
    // default config is the one whose numbers are in the device's own rate.
    assert_eq!(declared_range(Some((480, 480)), [(80, 80), (480, 480)]), Some((480, 480)));
    // A device whose default config says nothing keeps a supported config's range, and one
    // that says nothing anywhere reports none.
    assert_eq!(declared_range(None, [(80, 80)]), Some((80, 80)));
    assert_eq!(declared_range(None, []), None);
    assert_eq!(declared_range(Some((256, 512)), []), Some((256, 512)));
}

#[test]
fn a_wanted_buffer_fits_the_nearest_allowed_one() {
    let allowed = [64, 128, 256, 512, 1024];
    assert_eq!(fit_buffer(512, &allowed), 512);
    assert_eq!(fit_buffer(300, &allowed), 256);
    assert_eq!(fit_buffer(384, &allowed), 512, "a tie goes to the larger");
    assert_eq!(fit_buffer(4096, &allowed), 1024);
    assert_eq!(fit_buffer(10, &allowed), 64);
    assert_eq!(fit_buffer(10, &[]), MIN_BUFFER_FRAMES);
    assert_eq!(fit_buffer(100_000, &[]), MAX_BUFFER_FRAMES);
}

#[test]
fn exclusive_wasapi_caps_the_period_and_the_engine_block_follows_it() {
    assert_eq!(exclusive_buffer(None), None);
    assert_eq!(exclusive_buffer(Some(256)), Some(256));
    assert_eq!(exclusive_buffer(Some(4096)), Some(WASAPI_MOST_FRAMES), "a size saved before the cap comes down");
    assert_eq!(exclusive_buffer(Some(16)), Some(MIN_BUFFER_FRAMES));
    // The engine renders in the capped period, not in the size that was saved.
    assert_eq!(crate::block_for(exclusive_buffer(Some(4096)), None), WASAPI_MOST_FRAMES as usize);
}

#[test]
fn wasapi_shared_takes_any_size_for_the_engine_and_asks_the_device_for_none() {
    // Windows plays shared WASAPI in its own period (480 frames here), whatever is asked.
    assert_eq!(fit_for_host("WASAPI", 256, Some((480, 480))), (256, None));
    assert_eq!(fit_for_host("WASAPI", 10_000, Some((480, 480))), (2048, None), "no more than WASAPI_MOST_FRAMES");
    assert_eq!(offered_buffers("WASAPI", Some((480, 480))), vec![64, 128, 256, 512, 1024, 2048]);
    // A driver that sets its buffer (ASIO) gets the nearest size it allows.
    assert_eq!(fit_for_host("ASIO", 300, Some((64, 2048))), (256, Some(256)));
    assert_eq!(fit_for_host("ASIO", 300, None), (256, None));
    assert_eq!(offered_buffers("ASIO", Some((256, 512))), vec![256, 512]);
}

#[test]
fn a_driver_is_never_asked_for_a_size_outside_its_range() {
    // The driver's own range caps what is asked for, and what is offered.
    assert_eq!(fit_for_host("ASIO", 4096, Some((32, 2048))), (2048, Some(2048)));
    assert_eq!(offered_buffers("ASIO", Some((32, 2048))), vec![64, 128, 256, 512, 1024, 2048]);
    // A driver that declares nothing keeps its own size; the engine's block is ours.
    assert_eq!(fit_for_host("ASIO", 512, Some((0, 0))), (512, None));
    assert_eq!(fit_for_host("ASIO", 512, Some((2048, 64))), (512, None));
    // A declared range the engine cannot render in: nothing is asked for, the block is ours.
    assert_eq!(fit_for_host("ASIO", 4096, Some((8192, 16384))), (4096, None));
    assert_eq!(fit_for_host("ASIO", 4096, Some((8, 32))), (4096, None));
    assert!(offered_buffers("ASIO", Some((8192, 16384))).is_empty());
}

#[test]
fn a_size_the_driver_will_not_take_is_said_out_loud() {
    let range = Some((32, 2048));
    // Fitted to the driver's range: the range and the size that plays are said.
    let notice = buffer_notice("ASIO", "Apollo", 4096, 2048, Some(2048), range).unwrap();
    assert!(notice.contains("32 to 2048") && notice.contains("2048 frames play"), "{notice}");
    // Had as asked: nothing to say.
    assert_eq!(buffer_notice("ASIO", "Apollo", 2048, 2048, Some(2048), range), None);
    // Sizes the engine cannot render in: the driver keeps its own.
    let notice = buffer_notice("ASIO", "Apollo", 4096, 4096, None, Some((8192, 16384))).unwrap();
    assert!(notice.contains("8192 to 16384") && notice.contains("keeps its own"), "{notice}");
    // A driver that declared nothing was asked for nothing; Windows sets WASAPI's buffer.
    assert_eq!(buffer_notice("ASIO", "Apollo", 4096, 4096, None, None), None);
    assert_eq!(buffer_notice("WASAPI", "Apollo", 4096, 4096, None, Some((480, 480))), None);
    // A driver that declared no range keeps its own size, so a block fitted to something
    // else than was asked for is said too, and a nonsense range is not quoted.
    let notice = buffer_notice("ASIO", "Apollo", 300, 256, None, None).unwrap();
    assert!(notice.contains("its own buffer size") && notice.contains("256-frame blocks"), "{notice}");
    assert_eq!(buffer_notice("ASIO", "Apollo", 256, 256, None, None), None);
    let notice = buffer_notice("ASIO", "Apollo", 300, 256, None, Some((0, 0))).unwrap();
    assert!(!notice.contains("0 to 0") && notice.contains("256-frame blocks"), "{notice}");
}

#[test]
fn sample_rates_come_from_the_ranges() {
    assert_eq!(sample_rates(&[(44_100, 44_100)]), vec![44_100]);
    assert_eq!(sample_rates(&[(44_100, 48_000)]), vec![44_100, 48_000]);
    assert_eq!(sample_rates(&[(8_000, 96_000), (44_100, 44_100)]).len(), 9);
    // A driver's odd fixed rate is kept; nonsense is not.
    assert_eq!(sample_rates(&[(47_999, 47_999)]), vec![47_999]);
    assert!(sample_rates(&[(0, 0), (u32::MAX, u32::MAX)]).is_empty());
    assert!(sample_rates(&[(96_000, 44_100)]).is_empty(), "a reversed range holds nothing");
    assert!(sample_rates(&[]).is_empty());
}

#[test]
fn names_match_without_case() {
    assert!(same_name("Speakers (Universal Audio Thunderbolt WDM)", " speakers (universal audio thunderbolt wdm)"));
    assert!(!same_name("Speakers", "Speakers 2"));
}

/// Listing works on any machine, with or without devices, and says something about each
/// host.
#[test]
fn listing_the_hosts_never_fails() {
    let hosts = list_hosts();
    for host in &hosts {
        assert!(!host.name.is_empty());
        for device in &host.devices {
            assert!(device.buffer_sizes.windows(2).all(|w| w[0] < w[1]), "{}", device.name);
            assert!(
                device.buffer_sizes.iter().all(|n| (MIN_BUFFER_FRAMES..=MAX_BUFFER_FRAMES).contains(n)),
                "{}",
                device.name
            );
            // Where the host asks the driver for the size — every host but WASAPI, whose
            // shared mode plays the period Windows gives the device, so its list is the
            // engine's own sizes and no claim about the device — every size offered sits
            // inside the range the driver declared. A driver that declared nothing offered
            // the engine's own sizes instead, and none is asked of it.
            if host.name != "WASAPI" {
                assert!(
                    device.buffer_range.is_none()
                        || device.buffer_sizes.iter().all(|n| within(device.buffer_range, *n)),
                    "{}",
                    device.name
                );
            }
        }
    }
    assert!(hosts.iter().filter(|h| h.default).count() <= 1);
}

/// A device as the list sees it.
#[derive(Clone)]
struct Stand {
    name: Option<&'static str>,
    plays: bool,
    offers: Vec<Offer>,
    own: Option<Offer>,
}

impl Listed for Stand {
    fn name(&self) -> Option<String> {
        self.name.map(String::from)
    }

    fn plays(&self) -> bool {
        self.plays
    }

    fn offers(&self) -> Vec<Offer> {
        self.offers.clone()
    }

    fn own(&self) -> Option<Offer> {
        self.own
    }
}

fn offer(rate: u32, channels: u16) -> Offer {
    Offer { rates: (rate, rate), channels, buffer: None }
}

/// A Bluetooth endpoint can be there before it can say what it offers: it stays in the list, and
/// opening it is what says what is wrong.
#[test]
fn a_device_that_cannot_say_what_it_offers_is_still_listed() {
    let asleep = Stand { name: Some("Headphones"), plays: true, offers: Vec::new(), own: None };
    let listed = listed_outputs([asleep], "WASAPI", None);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Headphones");
    assert!(listed[0].sample_rates.is_empty());
    assert_eq!(listed[0].default_sample_rate, None);
    assert_eq!(listed[0].channels, 0);
}

/// Bluetooth headphones often take one rate, and an odd buffer: neither takes them off the list.
#[test]
fn a_device_with_one_rate_is_listed_with_that_rate() {
    let own = Offer { rates: (44_100, 44_100), channels: 2, buffer: Some((441, 441)) };
    let headphones = Stand { name: Some("Headphones"), plays: true, offers: vec![own], own: Some(own) };
    for host in ["WASAPI", "CoreAudio", "ALSA", "PulseAudio"] {
        let listed = listed_outputs([headphones.clone()], host, Some("Headphones"));
        assert_eq!(listed.len(), 1, "{host}");
        assert_eq!(listed[0].sample_rates, vec![44_100], "{host}");
        assert_eq!(listed[0].default_sample_rate, Some(44_100), "{host}");
        assert_eq!(listed[0].channels, 2, "{host}");
        assert!(listed[0].default, "{host}");
    }
}

#[test]
fn only_what_plays_and_has_a_name_is_left_in_the_list() {
    let microphone = Stand { name: Some("Microphone"), plays: false, offers: Vec::new(), own: None };
    let unplugged = Stand { name: None, plays: true, offers: vec![offer(48_000, 2)], own: None };
    let speakers = Stand { name: Some("Speakers"), plays: true, offers: vec![offer(48_000, 2)], own: None };
    let listed = listed_outputs([microphone, unplugged, speakers], "WASAPI", None);
    let names: Vec<&str> = listed.iter().map(|device| device.name.as_str()).collect();
    assert_eq!(names, ["Speakers"]);
    assert!(!listed[0].default);
}

/// A sound server offers every channel count at every rate: the list says what the device is, so
/// a pair of headphones offers no outputs 3/4 for the cue.
#[test]
fn a_sound_server_lists_the_devices_own_channels_and_common_rates() {
    let any_format = |channels: u16| Offer { rates: (1, 384_000), channels, buffer: Some((1, 500_000)) };
    let offers: Vec<Offer> = (1..32).map(any_format).collect();
    let own = Offer { rates: (48_000, 48_000), channels: 2, buffer: Some((1, 500_000)) };
    let sink = Stand { name: Some("Headphones"), plays: true, offers, own: Some(own) };
    let listed = listed_outputs([sink], "PulseAudio", None);
    assert_eq!(listed[0].channels, 2);
    assert!(listed[0].sample_rates.contains(&44_100) && listed[0].sample_rates.contains(&48_000));
    assert_eq!(listed[0].buffer_sizes, BUFFER_SIZES.to_vec(), "nothing outside what the engine renders in");
    // Elsewhere the most it offers counts: an interface's own format need not be its widest.
    assert_eq!(listed_channels("WASAPI", Some(2), 8), 8);
    assert_eq!(listed_channels("WASAPI", Some(8), 2), 8);
    assert_eq!(listed_channels("PulseAudio", None, 31), 31, "a server that gives no default config");
}

#[test]
fn only_a_sound_server_is_asked_for_the_engines_block_unasked() {
    assert_eq!(wanted_buffer("PulseAudio", None, 1024), Some(1024));
    assert_eq!(wanted_buffer("PulseAudio", Some(256), 1024), Some(256));
    assert_eq!(wanted_buffer("WASAPI", None, 1024), None);
    assert_eq!(wanted_buffer("ALSA", None, 1024), None);
    assert_eq!(wanted_buffer("ALSA", Some(256), 1024), Some(256));
}

fn driver(min: u32, max: u32, preferred: u32, granularity: i32) -> DriverBuffers {
    DriverBuffers::new(min, max, preferred, granularity)
}

/// A driver whose size is set in its own control panel answers with that one size as smallest,
/// largest and preferred: the list holds it alone, and it plays by default.
#[test]
fn a_driver_fixed_to_its_control_panels_size_offers_that_size_by_default() {
    let fixed = driver(128, 128, 128, 0);
    assert_eq!(fixed.sizes(), vec![128]);
    assert_eq!(fixed.default_size(), Some(128));
    assert_eq!(fit_to_driver(128, &fixed), (128, Some(128)));
    // A size saved before the control panel changed comes to the driver's one size.
    assert_eq!(fit_to_driver(2048, &fixed), (128, Some(128)));
    // A size outside what the engine renders in is asked for no size: the driver keeps its own.
    let tiny = driver(32, 32, 32, 0);
    assert!(tiny.sizes().is_empty());
    assert_eq!(tiny.default_size(), None);
    assert_eq!(fit_to_driver(1024, &tiny).1, None);
}

/// A driver answering one size with a granularity of -1 or a step, as real drivers fixed to their
/// control panel's size do, takes that size even when it is no power of two or step: it is the
/// only size the driver plays, and the engine renders in it.
#[test]
fn a_driver_answering_one_size_takes_it_whatever_granularity_it_states() {
    for granularity in [-1, 0, 48, -2] {
        let fixed = driver(480, 480, 480, granularity);
        assert_eq!(fixed.sizes(), vec![480], "granularity {granularity}");
        assert_eq!(fixed.default_size(), Some(480), "granularity {granularity}");
        assert_eq!(fit_to_driver(1024, &fixed), (480, Some(480)), "granularity {granularity}");
    }
    let fixed = driver(480, 480, 480, -1);
    let listed = listed_outputs([Driven(fixed)], "ASIO", None);
    assert_eq!(listed[0].buffer_sizes, vec![480]);
    assert_eq!(listed[0].default_buffer, Some(480));
    let (block, asked) = fit_to_driver(256, &fixed);
    assert_eq!(
        driver_notice("Interface", 256, block, asked, &fixed).as_deref(),
        Some("Interface plays only 480 frames, the size set in its control panel")
    );
    // Still within what the engine renders in.
    assert!(driver(32, 32, 32, -1).sizes().is_empty());
}

#[test]
fn powers_of_two_are_every_power_in_the_drivers_range() {
    let powers = driver(32, 8192, 256, -1);
    assert_eq!(powers.sizes(), vec![64, 128, 256, 512, 1024, 2048, 4096]);
    assert_eq!(powers.default_size(), Some(256));
    // Ends that are not powers of two are not sizes such a driver takes.
    assert_eq!(driver(96, 1000, 128, -1).sizes(), vec![128, 256, 512]);
    assert_eq!(fit_to_driver(300, &driver(96, 1000, 128, -1)), (256, Some(256)));
}

#[test]
fn a_stepped_driver_offers_every_size_on_its_steps() {
    // 48 to 480 in steps of 48: 96, 144 and 192 are offered, though they are no powers of two.
    let stepped = driver(48, 480, 144, 48);
    assert_eq!(stepped.sizes(), vec![96, 144, 192, 240, 288, 336, 384, 432, 480]);
    assert_eq!(stepped.default_size(), Some(144));
    assert_eq!(fit_to_driver(200, &stepped), (192, Some(192)));
    // A size off its steps is never asked for.
    assert_eq!(fit_to_driver(256, &stepped), (240, Some(240)));
    // Fine-grained drivers retain every permitted size, including past 64 entries.
    let fine = driver(16, 4096, 200, 8);
    assert_eq!(fine.sizes(), (64..=4096).step_by(8).collect::<Vec<_>>());
    // Steps that never land on a common size still give a list, every size on them.
    let coarse = driver(48, 8192, 192, 48);
    assert_eq!(coarse.sizes(), (96..=4080).step_by(48).collect::<Vec<_>>());
    assert!(coarse.sizes().iter().all(|n| (n - 48).is_multiple_of(48)));
    // A granularity below -1 is a step no size but the smallest is on, as the stream checks it.
    assert_eq!(driver(64, 2048, 64, -2).sizes(), vec![64]);
}

#[test]
fn an_unstated_granularity_offers_the_preferred_size_alone() {
    assert_eq!(driver(96, 2000, 333, 0).sizes(), vec![333]);
    assert_eq!(fit_to_driver(1024, &driver(96, 2000, 333, 0)), (333, Some(333)));
    // A preferred size outside the range says nothing: only the two ends it declared, never a
    // size between them it did not promise. With none chosen the one nearest the engine's block
    // plays, so the engine renders in the size the driver plays.
    let odd = driver(256, 2048, 4096, 0);
    assert_eq!(odd.sizes(), vec![256, 2048]);
    assert_eq!(odd.default_size(), Some(256));
    assert_eq!(plan_for_driver(None, &odd), Some((256, Some(256))));
}

#[test]
fn a_driver_answer_that_makes_no_sense_counts_as_none() {
    for broken in [driver(0, 0, 0, 0), driver(2048, 64, 128, -1)] {
        assert_eq!(broken.range(), None);
        assert!(broken.sizes().is_empty());
        assert_eq!(broken.default_size(), None);
        assert_eq!(fit_to_driver(512, &broken), (512, None), "never asked for a size it did not declare");
        assert_eq!(plan_for_driver(None, &broken), None, "it keeps its own and the engine its block");
    }
}

/// An ASIO device as the list reads it: its offers say only the range, the driver says the rest.
#[derive(Clone)]
struct Driven(DriverBuffers);

impl Listed for Driven {
    fn name(&self) -> Option<String> {
        Some("Interface".into())
    }

    fn plays(&self) -> bool {
        true
    }

    fn offers(&self) -> Vec<Offer> {
        vec![Offer { rates: (44_100, 44_100), channels: 2, buffer: self.0.range() }]
    }

    fn own(&self) -> Option<Offer> {
        self.offers().first().copied()
    }

    fn driver(&self) -> Option<DriverBuffers> {
        Some(self.0)
    }
}

#[test]
fn the_list_holds_what_the_asio_driver_allows_and_its_preferred_size() {
    let listed = listed_outputs([Driven(driver(128, 128, 128, 0))], "ASIO", None);
    assert_eq!(listed[0].buffer_sizes, vec![128]);
    assert_eq!(listed[0].buffer_range, Some((128, 128)));
    assert_eq!(listed[0].default_buffer, Some(128));
    let listed = listed_outputs([Driven(driver(32, 2048, 256, -1))], "ASIO", None);
    assert_eq!(listed[0].buffer_sizes, vec![64, 128, 256, 512, 1024, 2048]);
    assert_eq!(listed[0].default_buffer, Some(256));
    // Other hosts say no preferred size.
    let own = Offer { rates: (48_000, 48_000), channels: 2, buffer: Some((256, 512)) };
    let card = Stand { name: Some("Card"), plays: true, offers: vec![own], own: Some(own) };
    assert_eq!(listed_outputs([card], "ALSA", None)[0].default_buffer, None);
}

/// A driver whose preferred size is off its steps still gets a size asked of it with none chosen:
/// the one nearest the engine's block, so the block and the driver's buffer are the same size.
#[test]
fn with_no_usable_preferred_size_the_size_nearest_the_engine_block_plays() {
    let stepped = driver(96, 2016, 100, 96);
    assert_eq!(stepped.default_size(), Some(1056));
    assert_eq!(plan_for_driver(None, &stepped), Some((1056, Some(1056))));
    // A size chosen wins over the default.
    assert_eq!(plan_for_driver(Some(200), &stepped), Some((192, Some(192))));
}

#[test]
fn a_notice_says_what_the_driver_takes() {
    let fixed = driver(128, 128, 128, 0);
    let (block, asked) = fit_to_driver(2048, &fixed);
    assert_eq!(
        driver_notice("Interface", 2048, block, asked, &fixed).as_deref(),
        Some("Interface plays only 128 frames, the size set in its control panel")
    );
    let powers = driver(64, 2048, 256, -1);
    let (block, asked) = fit_to_driver(300, &powers);
    assert_eq!(
        driver_notice("Interface", 300, block, asked, &powers).as_deref(),
        Some("Interface takes 64 to 2048 frames in powers of two; 256 frames play")
    );
    assert_eq!(driver(48, 480, 144, 48).describe(), "48 to 480 frames in steps of 48");
    assert_eq!(driver(96, 2000, 333, 0).describe(), "only 333 frames");
    // What was chosen plays: nothing to say.
    let (block, asked) = fit_to_driver(256, &powers);
    assert_eq!(driver_notice("Interface", 256, block, asked, &powers), None);
    // A driver whose sizes the engine cannot render in keeps its own.
    let tiny = driver(32, 32, 32, 0);
    let (block, asked) = fit_to_driver(1024, &tiny);
    assert_eq!(
        driver_notice("Interface", 1024, block, asked, &tiny).as_deref(),
        Some("Interface takes only 32 frames, which Suhina cannot render in; it keeps its own buffer size")
    );
}

/// A driver whose answer makes no sense is listed with the engine's own sizes, as one that
/// declared nothing, and asked for none.
#[test]
fn a_broken_driver_is_listed_with_the_engines_sizes() {
    let listed = listed_outputs([Driven(driver(0, 0, 0, 0))], "ASIO", None);
    assert_eq!(listed[0].buffer_sizes, BUFFER_SIZES.to_vec());
    assert_eq!(listed[0].buffer_range, None);
    assert_eq!(listed[0].default_buffer, None);
}

/// The trace line names the driver's raw answer and what Suhina makes of it, so a log from a
/// driver that answers oddly says why a size played.
#[test]
fn the_trace_text_names_the_answer_and_the_sizes_made_from_it() {
    assert_eq!(
        driver(2048, 2048, 2048, 0).trace_text(),
        "min=2048 max=2048 preferred=2048 granularity=Unstated; sizes [2048], default Some(2048)"
    );
    assert_eq!(
        driver(64, 256, 128, -1).trace_text(),
        "min=64 max=256 preferred=128 granularity=PowersOfTwo; sizes [64, 128, 256], default Some(128)"
    );
}
