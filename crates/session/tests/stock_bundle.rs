// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

#[test]
fn every_stock_sd_imports_as_its_built_in_by_id() {
    let folder = std::env::temp_dir().join(format!(
        "suhina-stock-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    std::fs::create_dir(&folder).unwrap();
    let _cleanup = Cleanup(folder.clone());
    suhina_device_bundle::write_to(&folder).unwrap();
    for stock in suhina_device_bundle::DEVICES {
        let project = Project {
            tracks: vec![Track {
                content: ContentKinds { notes: stock.instrument, audio: !stock.instrument },
                channel: Some(ChannelId(1)),
                ..Track::new(TrackId(1), "Track")
            }],
            channels: vec![Channel::new(ChannelId(1), "Track")],
            ..Project::default()
        };
        let mut session = Session::new(project);
        let path = folder.join(format!("{}.sd", stock.id));
        session.execute("device.addSd", &json!({"track":1,"path":path})).unwrap();
        let device = &session.project().channels[0].devices[0];
        assert_eq!(device.kind, DeviceKind::Builtin(stock.id.into()));
        // The project keeps the built-in by its id; its definition comes from the bundle (ADR 0021,
        // amendment 3).
        assert!(device.extensions.0.iter().all(|e| e.key != "stock_sd"), "no stored definition");
        assert!(suhina_device_bundle::source(device).is_some(), "the bundle has its definition");
        let restored = suhina_project_file::from_json(&suhina_project_file::to_json(session.project())).unwrap();
        assert_eq!(&restored, session.project());
        if stock.id == "sampler" {
            let mut project = session.project().clone();
            project.channels[0].devices[0].set_sample_file("source.wav");
            session = Session::new(project);
            // A built-in's definition is always the bundle's (ADR 0021 amendment 3), so there is
            // nothing of it to update: device.updateSd refuses it and the sample stays.
            let refused = session.execute("device.updateSd", &json!({"track":1,"device":0,"path":path}));
            assert!(refused.is_err(), "{refused:?}");
            assert_eq!(session.project().channels[0].devices[0].sample_file(), Some("source.wav"));
        }
        std::fs::remove_file(path).unwrap();
    }
}
