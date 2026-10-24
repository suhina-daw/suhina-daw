// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

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
