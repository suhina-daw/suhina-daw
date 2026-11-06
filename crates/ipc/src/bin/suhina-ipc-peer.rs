// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stand-in for the plug-in host, used by the `ipc` tests: it opens the link named on its
//! command line and serves blocks, halving every sample.
//!
//! `suhina-ipc-peer <address> <mode>`, where mode is `serve` (until no block comes for two
//! seconds), `crash` (exits in the middle of block 4, without answering) or `hang` (stops
//! answering in the middle of block 4).
//!
//! For the tests of leftover memory: `suhina-ipc-peer make` makes a link and an audio source,
//! prints their addresses, one a line, and keeps them until its standard input closes;
//! `suhina-ipc-peer idle` prints its process id and waits the same way, making nothing.

use std::io::{Read, Write};
use std::time::Duration;

use suhina_ipc::{AudioSource, Link, Served};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("make") => return make(),
        Some("idle") => return idle(),
        _ => {}
    }
    let (Some(address), Some(mode)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: suhina-ipc-peer <address> <serve|crash|hang> | make | idle");
        std::process::exit(2);
    };
    let mut served = match Served::open(address) {
        Ok(served) => served,
        Err(e) => {
            eprintln!("suhina-ipc-peer: {e}");
            std::process::exit(1);
        }
    };
    let mut block = 0u32;
    loop {
        let fails_now = block == 4 && mode != "serve";
        let served_one = served.serve(Duration::from_secs(2), |input, output| {
            if fails_now {
                if mode == "crash" {
                    std::process::exit(3);
                }
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            }
            for (o, i) in output.iter_mut().zip(input) {
                *o = i * 0.5;
            }
        });
        if !served_one {
            return;
        }
        block += 1;
    }
}

fn make() {
    let made = Link::create(2, 64).and_then(|link| Ok((link, AudioSource::from_interleaved(&[0.5; 64], 2, 48_000.0)?)));
    let (link, source) = match made {
        Ok(made) => made,
        Err(e) => {
            eprintln!("suhina-ipc-peer: {e}");
            std::process::exit(1);
        }
    };
    println!("{}\n{}", link.address(), source.address());
    wait_for_stdin();
}

fn idle() {
    println!("{}", std::process::id());
    wait_for_stdin();
}

/// Until the parent closes standard input (or the process is killed).
fn wait_for_stdin() {
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
}
